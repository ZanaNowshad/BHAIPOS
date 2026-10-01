use super::*;

impl Store {
    pub fn create_supplier(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        supplier_id: Uuid,
        name: &str,
        phone: Option<&str>,
        payment_terms_days: Option<i64>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "procurement.manage",
        )?;
        if name.trim().is_empty() || payment_terms_days.is_some_and(|days| days < 0) {
            return Err(StoreError::Validation(
                "invalid supplier identity or terms".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO suppliers(id,tenant_id,name,phone,payment_terms_days,active,created_at) VALUES(?1,?2,?3,?4,?5,1,?6)",
            params![supplier_id.to_string(), context.tenant_id.to_string(), name.trim(), phone.map(str::trim), payment_terms_days, now.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn set_supplier_product(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        supplier_id: Uuid,
        product_id: ProductId,
        supplier_sku: Option<&str>,
        case_size: QuantityMilli,
        moq: QuantityMilli,
        current_cost: Money,
        lead_time_days: Option<i64>,
        preferred: bool,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "procurement.manage",
        )?;
        if case_size.0 <= 0
            || moq.0 <= 0
            || current_cost.0 < 0
            || lead_time_days.is_some_and(|days| days < 0)
        {
            return Err(StoreError::Validation(
                "invalid supplier product terms".into(),
            ));
        }
        let supplier_ok: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let product_ok: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM products WHERE id=?1 AND tenant_id=?2",
                params![product_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() || product_ok.is_none() {
            return Err(StoreError::Authorization(
                "supplier product tenant mismatch",
            ));
        }
        let existing: Option<String> = self.conn.query_row(
            "SELECT id FROM supplier_products WHERE tenant_id=?1 AND supplier_id=?2 AND product_id=?3",
            params![context.tenant_id.to_string(), supplier_id.to_string(), product_id.to_string()], |row| row.get(0),
        ).optional()?;
        let id = existing
            .and_then(|value| Uuid::parse_str(&value).ok())
            .unwrap_or_else(Uuid::new_v4);
        self.conn.execute(
            "INSERT INTO supplier_products(id,tenant_id,supplier_id,product_id,supplier_sku,case_size_milli,moq_milli,current_cost_fils,lead_time_days,preferred) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(tenant_id,supplier_id,product_id) DO UPDATE SET supplier_sku=excluded.supplier_sku,case_size_milli=excluded.case_size_milli,moq_milli=excluded.moq_milli,current_cost_fils=excluded.current_cost_fils,lead_time_days=excluded.lead_time_days,preferred=excluded.preferred",
            params![id.to_string(), context.tenant_id.to_string(), supplier_id.to_string(), product_id.to_string(), supplier_sku.map(str::trim), case_size.0, moq.0, current_cost.0, lead_time_days, preferred as i32],
        )?;
        Ok(id)
    }

    pub fn create_purchase_order(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        supplier_id: Uuid,
        lines: &[PurchaseOrderLineInput],
        now: DateTime<Utc>,
    ) -> Result<PurchaseOrderResult, StoreError> {
        self.validate_local_session(context, user)?;
        if lines.is_empty() {
            return Err(StoreError::Validation("purchase order has no lines".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(supplier_id, lines))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_CREATE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "procurement.manage",
        )?;
        let supplier_ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let mut calculated = Vec::with_capacity(lines.len());
        let mut subtotal = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut seen = HashSet::new();
        for line in lines {
            if line.quantity.0 <= 0
                || line.unit_cost.0 < 0
                || !(0..=10_000).contains(&line.tax_rate_bps)
                || !seen.insert(line.product_id)
            {
                return Err(StoreError::Validation(
                    "invalid or duplicate purchase order line".into(),
                ));
            }
            Self::assert_product(&tx, context.tenant_id, line.product_id)?;
            let base = price_times_quantity(line.unit_cost, line.quantity)?;
            let breakdown = TaxRule {
                category: TaxCategory::StandardRated,
                rate_bps: line.tax_rate_bps,
                inclusive: false,
            }
            .calculate(base)?;
            subtotal = subtotal.checked_add(base)?;
            tax = tax.checked_add(breakdown.tax)?;
            calculated.push((line, base.checked_add(breakdown.tax)?));
        }
        let total = subtotal.checked_add(tax)?;
        let purchase_order_id = Uuid::new_v4();
        let po_number = format!(
            "PO-{}-{}",
            now.format("%Y%m%d%H%M%S"),
            &purchase_order_id.simple().to_string()[..8]
        );
        tx.execute(
            "INSERT INTO purchase_orders(id,tenant_id,branch_id,supplier_id,po_number,status,subtotal_fils,tax_fils,total_fils,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,'DRAFT',?6,?7,?8,?9,?10)",
            params![purchase_order_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), supplier_id.to_string(), po_number, subtotal.0, tax.0, total.0, user.to_string(), now.to_rfc3339()],
        )?;
        for (line, line_total) in calculated {
            tx.execute(
                "INSERT INTO purchase_order_lines(id,po_id,product_id,ordered_qty_milli,unit_cost_fils,tax_rate_bps,line_total_fils) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![Uuid::new_v4().to_string(), purchase_order_id.to_string(), line.product_id.to_string(), line.quantity.0, line.unit_cost.0, line.tax_rate_bps, line_total.0],
            )?;
        }
        let result = PurchaseOrderResult {
            purchase_order_id,
            po_number,
            status: "DRAFT".into(),
            total,
        };
        Self::append_purchase_order_event(
            &tx,
            context,
            user,
            purchase_order_id,
            operation_id,
            "CREATED",
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_CREATE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "PURCHASE_ORDER_CREATED",
            "purchase_order",
            &purchase_order_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn progress_purchase_order(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        purchase_order_id: Uuid,
        operation_id: OperationId,
        target: &str,
        now: DateTime<Utc>,
    ) -> Result<PurchaseOrderResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(target, "APPROVED" | "ORDERED" | "CANCELLED") {
            return Err(StoreError::Validation(
                "invalid purchase order transition".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(purchase_order_id, target))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_PROGRESS",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let permission = if target == "APPROVED" {
            "procurement.approve"
        } else {
            "procurement.manage"
        };
        Self::assert_permission(&tx, context.tenant_id, context.branch_id, user, permission)?;
        let current: Option<(String, String, i64)> = tx.query_row(
            "SELECT po_number,status,total_fils FROM purchase_orders WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
            params![purchase_order_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let (po_number, status, total) = current.ok_or(StoreError::NotFound("purchase order"))?;
        if !matches!(
            (status.as_str(), target),
            ("DRAFT", "APPROVED")
                | ("APPROVED", "ORDERED")
                | ("DRAFT", "CANCELLED")
                | ("APPROVED", "CANCELLED")
        ) {
            return Err(StoreError::Conflict(
                "purchase order transition is not allowed".into(),
            ));
        }
        tx.execute(
            "UPDATE purchase_orders SET status=?2,approved_by_user_id=CASE WHEN ?2='APPROVED' THEN ?3 ELSE approved_by_user_id END,ordered_at=CASE WHEN ?2='ORDERED' THEN ?4 ELSE ordered_at END WHERE id=?1",
            params![purchase_order_id.to_string(), target, user.to_string(), now.to_rfc3339()],
        )?;
        let result = PurchaseOrderResult {
            purchase_order_id,
            po_number,
            status: target.into(),
            total: Money(total),
        };
        Self::append_purchase_order_event(
            &tx,
            context,
            user,
            purchase_order_id,
            operation_id,
            target,
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_PROGRESS",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "PURCHASE_ORDER_PROGRESS",
            "purchase_order",
            &purchase_order_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn receive_purchase_order(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        purchase_order_id: Uuid,
        operation_id: OperationId,
        centre_id: Uuid,
        lines: &[PurchaseOrderReceiptLineInput],
        supplier_document_no: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<PurchaseReceiptResult, StoreError> {
        self.validate_local_session(context, user)?;
        if lines.is_empty() {
            return Err(StoreError::Validation(
                "purchase order receipt has no lines".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            purchase_order_id,
            centre_id,
            lines,
            supplier_document_no.map(str::trim),
        ))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_RECEIVE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.receive",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, centre_id)?;
        let header: Option<(String, String)> = tx.query_row(
            "SELECT supplier_id,status FROM purchase_orders WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
            params![purchase_order_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let (supplier, status) = header.ok_or(StoreError::NotFound("purchase order"))?;
        if !matches!(
            status.as_str(),
            "APPROVED" | "ORDERED" | "PARTIALLY_RECEIVED"
        ) {
            return Err(StoreError::Conflict(
                "purchase order is not receivable".into(),
            ));
        }
        let receipt_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO goods_receipts(id,tenant_id,branch_id,supplier_id,po_id,centre_id,operation_id,status,device_id,received_by_user_id,supplier_document_no,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'POSTED',?8,?9,?10,?11)",
            params![receipt_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), supplier, purchase_order_id.to_string(), centre_id.to_string(), operation_id.to_string(), context.device_id.to_string(), user.to_string(), supplier_document_no.map(str::trim), now.to_rfc3339()],
        )?;
        let mut accepted_total = 0_i64;
        let mut cost_variance = Money::ZERO;
        let mut discrepancy = false;
        let mut seen = HashSet::new();
        for input in lines {
            if !seen.insert(input.purchase_order_line_id)
                || input.received_quantity.0 <= 0
                || input.rejected_quantity.0 < 0
                || input.damaged_quantity.0 < 0
                || input.unit_cost.0 < 0
            {
                return Err(StoreError::Validation(
                    "invalid or duplicate purchase receipt line".into(),
                ));
            }
            let unavailable = input
                .rejected_quantity
                .0
                .checked_add(input.damaged_quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let accepted = input
                .received_quantity
                .0
                .checked_sub(unavailable)
                .ok_or_else(|| {
                    StoreError::Validation(
                        "rejected and damaged quantity exceed received quantity".into(),
                    )
                })?;
            let row: Option<(String, i64, i64, i64)> = tx.query_row(
                "SELECT product_id,ordered_qty_milli,received_qty_milli,unit_cost_fils FROM purchase_order_lines WHERE id=?1 AND po_id=?2",
                params![input.purchase_order_line_id.to_string(), purchase_order_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional()?;
            let (product, ordered, received, ordered_cost) =
                row.ok_or(StoreError::NotFound("purchase order line"))?;
            if received
                .checked_add(input.received_quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?
                > ordered
            {
                return Err(StoreError::Conflict(
                    "purchase receipt exceeds ordered quantity".into(),
                ));
            }
            let product_id =
                ProductId(Uuid::parse_str(&product).map_err(|_| {
                    StoreError::Validation("invalid purchase order product".into())
                })?);
            let lot_id = if input.lot_number.is_some() || input.expires_on.is_some() {
                let id = Uuid::new_v4();
                tx.execute("INSERT INTO inventory_lots(id,tenant_id,product_id,supplier_id,lot_number,expires_on,status,unit_cost_fils,received_at) VALUES(?1,?2,?3,?4,?5,?6,'ACTIVE',?7,?8)", params![id.to_string(), context.tenant_id.to_string(), product, supplier, input.lot_number, input.expires_on, input.unit_cost.0, now.to_rfc3339()])?;
                Some(id)
            } else {
                None
            };
            let receipt_line_id = Uuid::new_v4();
            let line_discrepancy = unavailable > 0 || input.unit_cost.0 != ordered_cost;
            discrepancy |= line_discrepancy;
            tx.execute(
                "INSERT INTO goods_receipt_lines(id,receipt_id,po_line_id,product_id,lot_id,ordered_qty_milli,received_qty_milli,rejected_qty_milli,damaged_qty_milli,unit_cost_fils,discrepancy_type) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![receipt_line_id.to_string(), receipt_id.to_string(), input.purchase_order_line_id.to_string(), product, lot_id.map(|id| id.to_string()), ordered, input.received_quantity.0, input.rejected_quantity.0, input.damaged_quantity.0, input.unit_cost.0, if line_discrepancy { Some("QUANTITY_OR_COST_MISMATCH") } else { None }],
            )?;
            if accepted > 0 {
                Self::append_inventory_effect(
                    &tx,
                    context,
                    user,
                    operation_id,
                    centre_id,
                    product_id,
                    QuantityMilli(accepted),
                    input.unit_cost,
                    "RECEIVING",
                    "GOODS_RECEIPT_LINE",
                    &receipt_line_id.to_string(),
                    lot_id,
                    now,
                )?;
                accepted_total = accepted_total
                    .checked_add(accepted)
                    .ok_or(bhaipos_core::MoneyError::Overflow)?;
                cost_variance = cost_variance.checked_add(price_times_quantity(
                    input.unit_cost.checked_sub(Money(ordered_cost))?,
                    QuantityMilli(accepted),
                )?)?;
                let valuation = Self::inventory_valuation_tx(
                    &tx,
                    context.tenant_id,
                    context.branch_id,
                    centre_id,
                    product_id,
                )?;
                tx.execute(
                    "UPDATE products SET current_cost_fils=?3 WHERE id=?1 AND tenant_id=?2",
                    params![
                        product,
                        context.tenant_id.to_string(),
                        valuation.weighted_average_cost.0
                    ],
                )?;
                tx.execute("INSERT INTO cost_history(id,tenant_id,product_id,cost_fils,effective_from,source,created_at) VALUES(?1,?2,?3,?4,?5,'PURCHASE_ORDER_RECEIVING',?5)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), product, valuation.weighted_average_cost.0, now.to_rfc3339()])?;
            }
            tx.execute("UPDATE purchase_order_lines SET received_qty_milli=received_qty_milli+?2 WHERE id=?1", params![input.purchase_order_line_id.to_string(), input.received_quantity.0])?;
        }
        let remaining: i64 = tx.query_row("SELECT COUNT(*) FROM purchase_order_lines WHERE po_id=?1 AND received_qty_milli<ordered_qty_milli", params![purchase_order_id.to_string()], |row| row.get(0))?;
        let next = if remaining == 0 {
            "RECEIVED"
        } else {
            "PARTIALLY_RECEIVED"
        };
        tx.execute(
            "UPDATE purchase_orders SET status=?2 WHERE id=?1",
            params![purchase_order_id.to_string(), next],
        )?;
        if discrepancy {
            tx.execute(
                "UPDATE goods_receipts SET status='REQUIRES_REVIEW' WHERE id=?1",
                params![receipt_id.to_string()],
            )?;
        }
        let result = PurchaseReceiptResult {
            receipt_id,
            purchase_order_id,
            status: next.into(),
            accepted_quantity: QuantityMilli(accepted_total),
            cost_variance,
        };
        Self::append_purchase_order_event(
            &tx,
            context,
            user,
            purchase_order_id,
            operation_id,
            "RECEIVED",
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PURCHASE_ORDER_RECEIVE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "PURCHASE_ORDER_RECEIVED",
            "goods_receipt",
            &receipt_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "inventory_receipt",
            &receipt_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn post_supplier_invoice(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        supplier_id: Uuid,
        invoice_number: &str,
        invoice_date: &str,
        due_date: Option<&str>,
        purchase_order_id: Option<Uuid>,
        goods_receipt_id: Option<Uuid>,
        lines: &[SupplierInvoiceLineInput],
        now: DateTime<Utc>,
    ) -> Result<SupplierInvoiceResult, StoreError> {
        self.validate_local_session(context, user)?;
        if invoice_number.trim().is_empty() || lines.is_empty() {
            return Err(StoreError::Validation(
                "supplier invoice number and lines are required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            supplier_id,
            invoice_number.trim(),
            invoice_date,
            due_date,
            purchase_order_id,
            goods_receipt_id,
            lines,
        ))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "SUPPLIER_INVOICE_POST",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "supplier.invoice.post",
        )?;
        let supplier_ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let mut subtotal = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut calculated = Vec::new();
        for line in lines {
            if line.description.trim().is_empty()
                || line.quantity.0 <= 0
                || line.unit_cost.0 < 0
                || line.tax.0 < 0
            {
                return Err(StoreError::Validation(
                    "invalid supplier invoice line".into(),
                ));
            }
            if let Some(product) = line.product_id {
                Self::assert_product(&tx, context.tenant_id, product)?;
            }
            let base = price_times_quantity(line.unit_cost, line.quantity)?;
            subtotal = subtotal.checked_add(base)?;
            tax = tax.checked_add(line.tax)?;
            calculated.push((line, base.checked_add(line.tax)?));
        }
        let total = subtotal.checked_add(tax)?;
        let invoice_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO supplier_invoices(id,tenant_id,branch_id,supplier_id,invoice_number,invoice_date,due_date,po_id,goods_receipt_id,subtotal_fils,tax_fils,total_fils,amount_paid_fils,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0,'OPEN',?13)",
            params![invoice_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), supplier_id.to_string(), invoice_number.trim(), invoice_date, due_date, purchase_order_id.map(|id| id.to_string()), goods_receipt_id.map(|id| id.to_string()), subtotal.0, tax.0, total.0, now.to_rfc3339()],
        )?;
        for (line, line_total) in calculated {
            tx.execute("INSERT INTO supplier_invoice_lines(id,invoice_id,product_id,description,quantity_milli,unit_cost_fils,tax_fils,line_total_fils) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![Uuid::new_v4().to_string(), invoice_id.to_string(), line.product_id.map(|id| id.to_string()), line.description.trim(), line.quantity.0, line.unit_cost.0, line.tax.0, line_total.0])?;
        }
        Self::append_supplier_ledger(
            &tx,
            context.tenant_id,
            context.branch_id,
            supplier_id,
            "INVOICE",
            "SUPPLIER_INVOICE",
            &invoice_id.to_string(),
            total,
            Money::ZERO,
            operation_id,
            invoice_date,
            None,
        )?;
        let result = SupplierInvoiceResult {
            invoice_id,
            total,
            status: "OPEN".into(),
        };
        Self::append_supplier_invoice_event(
            &tx,
            context,
            user,
            invoice_id,
            operation_id,
            "POSTED",
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "SUPPLIER_INVOICE_POST",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SUPPLIER_INVOICE_POSTED",
            "supplier_invoice",
            &invoice_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn pay_supplier(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        supplier_id: Uuid,
        method: &str,
        amount: Money,
        reference: Option<&str>,
        allocations: &[SupplierPaymentAllocationInput],
        now: DateTime<Utc>,
    ) -> Result<SupplierPaymentResult, StoreError> {
        self.validate_local_session(context, user)?;
        if amount.0 <= 0
            || !matches!(
                method,
                "CASH" | "BANK_TRANSFER" | "BENEFIT_PAY" | "CHEQUE" | "OTHER"
            )
        {
            return Err(StoreError::Validation("invalid supplier payment".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            supplier_id,
            method,
            amount,
            reference.map(str::trim),
            allocations,
        ))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "SUPPLIER_PAYMENT_POST",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "supplier.payment.post",
        )?;
        let supplier_ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let mut allocated = Money::ZERO;
        let mut seen = HashSet::new();
        for allocation in allocations {
            if allocation.amount.0 <= 0 || !seen.insert(allocation.invoice_id) {
                return Err(StoreError::Validation(
                    "invalid or duplicate supplier payment allocation".into(),
                ));
            }
            let invoice: Option<(i64, i64)> = tx.query_row("SELECT total_fils,amount_paid_fils FROM supplier_invoices WHERE id=?1 AND tenant_id=?2 AND supplier_id=?3 AND status IN ('OPEN','PARTIALLY_PAID')", params![allocation.invoice_id.to_string(), context.tenant_id.to_string(), supplier_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
            let (total, paid) = invoice.ok_or(StoreError::Conflict(
                "supplier invoice is not payable".into(),
            ))?;
            if allocation.amount.0
                > total
                    .checked_sub(paid)
                    .ok_or(bhaipos_core::MoneyError::Overflow)?
            {
                return Err(StoreError::Conflict(
                    "supplier payment allocation exceeds invoice balance".into(),
                ));
            }
            allocated = allocated.checked_add(allocation.amount)?;
        }
        if allocated.0 > amount.0 {
            return Err(StoreError::Validation(
                "supplier allocations exceed payment".into(),
            ));
        }
        let payment_id = Uuid::new_v4();
        tx.execute("INSERT INTO supplier_payments(id,tenant_id,branch_id,supplier_id,operation_id,device_id,user_id,method,amount_fils,reference,paid_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![payment_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), supplier_id.to_string(), operation_id.to_string(), context.device_id.to_string(), user.to_string(), method, amount.0, reference.map(str::trim), now.to_rfc3339()])?;
        for allocation in allocations {
            tx.execute("INSERT INTO supplier_payment_allocations(payment_id,invoice_id,amount_fils) VALUES(?1,?2,?3)", params![payment_id.to_string(), allocation.invoice_id.to_string(), allocation.amount.0])?;
            tx.execute("UPDATE supplier_invoices SET amount_paid_fils=amount_paid_fils+?2,status=CASE WHEN amount_paid_fils+?2=total_fils THEN 'PAID' ELSE 'PARTIALLY_PAID' END WHERE id=?1", params![allocation.invoice_id.to_string(), allocation.amount.0])?;
            Self::append_supplier_invoice_event(
                &tx,
                context,
                user,
                allocation.invoice_id,
                operation_id,
                "PAYMENT_ALLOCATED",
                allocation,
                now,
            )?;
        }
        Self::append_supplier_ledger(
            &tx,
            context.tenant_id,
            context.branch_id,
            supplier_id,
            "PAYMENT",
            "SUPPLIER_PAYMENT",
            &payment_id.to_string(),
            Money::ZERO,
            amount,
            operation_id,
            &now.to_rfc3339(),
            reference.map(str::trim),
        )?;
        let result = SupplierPaymentResult {
            payment_id,
            amount,
            unallocated: amount.checked_sub(allocated)?,
        };
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "SUPPLIER_PAYMENT_POST",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SUPPLIER_PAYMENT_POSTED",
            "supplier_payment",
            &payment_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "supplier_payment",
            &payment_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn supplier_statement(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        supplier_id: Uuid,
        from: &str,
        to: &str,
    ) -> Result<SupplierStatement, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "supplier.statement.view",
        )?;
        let supplier_ok: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let opening: i64 = self.conn.query_row("SELECT COALESCE(SUM(debit_fils-credit_fils),0) FROM supplier_ledger WHERE tenant_id=?1 AND supplier_id=?2 AND occurred_at<?3", params![context.tenant_id.to_string(), supplier_id.to_string(), from], |row| row.get(0))?;
        let mut statement = self.conn.prepare("SELECT event_type,source_type,source_id,debit_fils,credit_fils,occurred_at FROM supplier_ledger WHERE tenant_id=?1 AND supplier_id=?2 AND occurred_at>=?3 AND occurred_at<=?4 ORDER BY occurred_at,id")?;
        let raw = statement
            .query_map(
                params![
                    context.tenant_id.to_string(),
                    supplier_id.to_string(),
                    from,
                    to
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut debits = Money::ZERO;
        let mut credits = Money::ZERO;
        let mut entries = Vec::new();
        for (event_type, source_type, source_id, debit, credit, occurred_at) in raw {
            debits = debits.checked_add(Money(debit))?;
            credits = credits.checked_add(Money(credit))?;
            entries.push(SupplierStatementEntry {
                event_type,
                source_type,
                source_id,
                debit: Money(debit),
                credit: Money(credit),
                occurred_at,
            });
        }
        let closing_balance = Money(opening).checked_add(debits)?.checked_sub(credits)?;
        Ok(SupplierStatement {
            opening_balance: Money(opening),
            debits,
            credits,
            closing_balance,
            entries,
        })
    }

    pub fn create_purchase_requisition(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        lines: &[(ProductId, QuantityMilli)],
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        if lines.is_empty() {
            return Err(StoreError::Validation(
                "purchase requisition has no lines".into(),
            ));
        }
        let normalized = lines
            .iter()
            .map(|(product, quantity)| (product.to_string(), quantity.0))
            .collect::<Vec<_>>();
        let digest = sha256_hex(&serde_json::to_vec(&(normalized, note.map(str::trim)))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "REQUISITION_CREATE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "procurement.manage",
        )?;
        let requisition_id = Uuid::new_v4();
        tx.execute("INSERT INTO purchase_requisitions(id,tenant_id,branch_id,status,requested_by_user_id,note,created_at) VALUES(?1,?2,?3,'DRAFT',?4,?5,?6)", params![requisition_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), user.to_string(), note.map(str::trim), now.to_rfc3339()])?;
        let mut seen = HashSet::new();
        for (product, quantity) in lines {
            if quantity.0 <= 0 || !seen.insert(*product) {
                return Err(StoreError::Validation(
                    "requisition lines require unique products and positive quantities".into(),
                ));
            }
            Self::assert_product(&tx, context.tenant_id, *product)?;
            tx.execute("INSERT INTO purchase_requisition_lines(id,requisition_id,product_id,requested_qty_milli) VALUES(?1,?2,?3,?4)", params![Uuid::new_v4().to_string(), requisition_id.to_string(), product.to_string(), quantity.0])?;
        }
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "REQUISITION_CREATE",
            &digest,
            &requisition_id,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "PURCHASE_REQUISITION_CREATED",
            "purchase_requisition",
            &requisition_id.to_string(),
            &serde_json::json!({"line_count": lines.len()}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(requisition_id)
    }

    pub fn dispatch_supplier_return(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        supplier_id: Uuid,
        centre_id: Uuid,
        reason: &str,
        lines: &[SupplierReturnLineInput],
        now: DateTime<Utc>,
    ) -> Result<SupplierReturnResult, StoreError> {
        self.validate_local_session(context, user)?;
        if reason.trim().is_empty() || lines.is_empty() {
            return Err(StoreError::Validation(
                "supplier return reason and lines are required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            supplier_id,
            centre_id,
            reason.trim(),
            lines,
        ))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "SUPPLIER_RETURN_DISPATCH",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "supplier.return",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, centre_id)?;
        let supplier_ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![supplier_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if supplier_ok.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let supplier_return_id = Uuid::new_v4();
        tx.execute("INSERT INTO supplier_returns(id,tenant_id,branch_id,supplier_id,status,reason,operation_id,created_by_user_id,device_id,created_at) VALUES(?1,?2,?3,?4,'DISPATCHED',?5,?6,?7,?8,?9)", params![supplier_return_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), supplier_id.to_string(), reason.trim(), operation_id.to_string(), user.to_string(), context.device_id.to_string(), now.to_rfc3339()])?;
        let mut value = Money::ZERO;
        let mut seen = HashSet::new();
        for line in lines {
            if line.quantity.0 <= 0
                || line.unit_cost.0 < 0
                || !seen.insert((line.product_id, line.lot_id))
            {
                return Err(StoreError::Validation(
                    "invalid or duplicate supplier return line".into(),
                ));
            }
            let valuation = Self::inventory_valuation_tx(
                &tx,
                context.tenant_id,
                context.branch_id,
                centre_id,
                line.product_id,
            )?;
            if valuation.quantity.0 < line.quantity.0 {
                return Err(StoreError::Conflict(
                    "insufficient stock for supplier return".into(),
                ));
            }
            value = value.checked_add(price_times_quantity(line.unit_cost, line.quantity)?)?;
            tx.execute("INSERT INTO supplier_return_lines(id,return_id,product_id,lot_id,quantity_milli,unit_cost_fils) VALUES(?1,?2,?3,?4,?5,?6)", params![Uuid::new_v4().to_string(), supplier_return_id.to_string(), line.product_id.to_string(), line.lot_id.map(|id| id.to_string()), line.quantity.0, line.unit_cost.0])?;
            Self::append_inventory_effect(
                &tx,
                context,
                user,
                operation_id,
                centre_id,
                line.product_id,
                QuantityMilli(-line.quantity.0),
                valuation.weighted_average_cost,
                "SUPPLIER_RETURN",
                "SUPPLIER_RETURN",
                &supplier_return_id.to_string(),
                line.lot_id,
                now,
            )?;
        }
        let result = SupplierReturnResult {
            supplier_return_id,
            status: "DISPATCHED".into(),
            value,
        };
        Self::append_supplier_return_event(
            &tx,
            context,
            user,
            supplier_return_id,
            operation_id,
            "DISPATCHED",
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "SUPPLIER_RETURN_DISPATCH",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SUPPLIER_RETURN_DISPATCHED",
            "supplier_return",
            &supplier_return_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn settle_supplier_return(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        supplier_return_id: Uuid,
        operation_id: OperationId,
        resolution: &str,
        now: DateTime<Utc>,
    ) -> Result<SupplierReturnResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(resolution, "CREDITED" | "REPLACED") {
            return Err(StoreError::Validation(
                "invalid supplier return resolution".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(supplier_return_id, resolution))?);
        if let Some(result) = self.load_procurement_operation(
            context.tenant_id,
            operation_id,
            "SUPPLIER_RETURN_SETTLE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "supplier.return",
        )?;
        let header: Option<(String, String)> = tx.query_row("SELECT supplier_id,status FROM supplier_returns WHERE id=?1 AND tenant_id=?2 AND branch_id=?3", params![supplier_return_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let (supplier, status) = header.ok_or(StoreError::NotFound("supplier return"))?;
        if status != "DISPATCHED" && status != "ACKNOWLEDGED" {
            return Err(StoreError::Conflict(
                "supplier return is not settleable".into(),
            ));
        }
        let return_values = {
            let mut statement = tx.prepare("SELECT quantity_milli,unit_cost_fils FROM supplier_return_lines WHERE return_id=?1")?;
            let rows = statement.query_map(params![supplier_return_id.to_string()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut value = Money::ZERO;
        for (quantity, unit_cost) in return_values {
            value = value.checked_add(price_times_quantity(
                Money(unit_cost),
                QuantityMilli(quantity),
            )?)?;
        }
        tx.execute(
            "UPDATE supplier_returns SET status=?2 WHERE id=?1",
            params![supplier_return_id.to_string(), resolution],
        )?;
        if resolution == "CREDITED" {
            let supplier_id = Uuid::parse_str(&supplier)
                .map_err(|_| StoreError::Validation("invalid supplier identity".into()))?;
            Self::append_supplier_ledger(
                &tx,
                context.tenant_id,
                context.branch_id,
                supplier_id,
                "RETURN_CREDIT",
                "SUPPLIER_RETURN",
                &supplier_return_id.to_string(),
                Money::ZERO,
                value,
                operation_id,
                &now.to_rfc3339(),
                None,
            )?;
        }
        let result = SupplierReturnResult {
            supplier_return_id,
            status: resolution.into(),
            value,
        };
        Self::append_supplier_return_event(
            &tx,
            context,
            user,
            supplier_return_id,
            operation_id,
            resolution,
            &result,
            now,
        )?;
        Self::record_procurement_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "SUPPLIER_RETURN_SETTLE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SUPPLIER_RETURN_SETTLED",
            "supplier_return",
            &supplier_return_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn load_procurement_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let global: Option<(String, String)> = self.conn.query_row("SELECT action,request_sha256 FROM idempotency_operations WHERE tenant_id=?1 AND operation_id=?2", params![tenant.to_string(), operation_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let expected = format!("PROCUREMENT:{action}");
        if let Some((stored_action, stored_digest)) = &global {
            if stored_action != &expected || stored_digest != digest {
                return Err(StoreError::Conflict(
                    "operation ID already bound to a different action or payload".into(),
                ));
            }
        }
        let row: Option<(String, String)> = self.conn.query_row("SELECT request_sha256,result_json FROM procurement_operation_results WHERE tenant_id=?1 AND operation_id=?2 AND action=?3", params![tenant.to_string(), operation_id.to_string(), action], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        match row {
            Some((stored, json)) if stored == digest => Ok(Some(serde_json::from_str(&json)?)),
            Some(_) => Err(StoreError::Conflict(
                "procurement operation ID reused with different payload".into(),
            )),
            None if global.is_some() => Err(StoreError::Conflict(
                "operation binding exists without procurement result; recovery review required"
                    .into(),
            )),
            None => Ok(None),
        }
    }

    fn record_procurement_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) VALUES(?1,?2,?3,?4,?5)", params![tenant.to_string(), operation_id.to_string(), format!("PROCUREMENT:{action}"), digest, now.to_rfc3339()])?;
        tx.execute("INSERT INTO procurement_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)", params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn append_purchase_order_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        purchase_order_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO purchase_order_events(id,tenant_id,purchase_order_id,event_type,operation_id,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), purchase_order_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), serde_json::to_string(evidence)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn append_supplier_invoice_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        invoice_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO supplier_invoice_events(id,tenant_id,supplier_invoice_id,event_type,operation_id,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), invoice_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), serde_json::to_string(evidence)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn append_supplier_return_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        supplier_return_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO supplier_return_events(id,tenant_id,supplier_return_id,event_type,operation_id,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), supplier_return_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), serde_json::to_string(evidence)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn append_supplier_ledger(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        supplier: Uuid,
        event_type: &str,
        source_type: &str,
        source_id: &str,
        debit: Money,
        credit: Money,
        operation_id: OperationId,
        occurred_at: &str,
        note: Option<&str>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO supplier_ledger(id,tenant_id,supplier_id,branch_id,event_type,source_type,source_id,debit_fils,credit_fils,occurred_at,operation_id,note) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", params![Uuid::new_v4().to_string(), tenant.to_string(), supplier.to_string(), branch.to_string(), event_type, source_type, source_id, debit.0, credit.0, occurred_at, operation_id.to_string(), note])?;
        Ok(())
    }
}
