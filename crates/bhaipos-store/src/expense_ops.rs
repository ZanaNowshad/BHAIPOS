#![allow(
    clippy::too_many_arguments,
    reason = "trusted expense commands keep authority, exact amounts, idempotency, and event time explicit"
)]

use super::*;

impl Store {
    pub fn create_expense_category(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        category_id: Uuid,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Validation(
                "expense category name is required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(category_id, name))?);
        if let Some(result) = self.load_expense_operation(
            context.tenant_id,
            operation_id,
            "CATEGORY_CREATE",
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
            "expense.manage",
        )?;
        tx.execute(
            "INSERT INTO expense_categories(id,tenant_id,name,active) VALUES(?1,?2,?3,1)",
            params![category_id.to_string(), context.tenant_id.to_string(), name],
        )?;
        Self::record_expense_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CATEGORY_CREATE",
            &digest,
            &category_id,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "EXPENSE_CATEGORY_CREATED",
            "expense_category",
            &category_id.to_string(),
            &serde_json::json!({"name": name}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(category_id)
    }

    pub fn create_expense(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        expense_id: Uuid,
        category_id: Uuid,
        description: &str,
        amount: Money,
        tax: Money,
        incurred_on: &str,
        now: DateTime<Utc>,
    ) -> Result<ExpenseResult, StoreError> {
        self.validate_local_session(context, user)?;
        let description = description.trim();
        if description.is_empty()
            || incurred_on.trim().is_empty()
            || amount.0 <= 0
            || tax.0 < 0
            || tax.0 > amount.0
        {
            return Err(StoreError::Validation("invalid expense".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            expense_id,
            category_id,
            description,
            amount,
            tax,
            incurred_on.trim(),
        ))?);
        if let Some(result) =
            self.load_expense_operation(context.tenant_id, operation_id, "CREATE", &digest)?
        {
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
            "expense.manage",
        )?;
        tx.execute("INSERT INTO expenses(id,tenant_id,branch_id,category_id,status,description,amount_fils,tax_fils,incurred_on,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,'DRAFT',?5,?6,?7,?8,?9,?10)", params![expense_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), category_id.to_string(), description, amount.0, tax.0, incurred_on.trim(), user.to_string(), now.to_rfc3339()])?;
        let result = ExpenseResult {
            expense_id,
            status: "DRAFT".into(),
            amount,
            tax,
        };
        Self::append_expense_event(
            &tx,
            context,
            user,
            expense_id,
            operation_id,
            "CREATED",
            &result,
            now,
        )?;
        Self::record_expense_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CREATE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "EXPENSE_CREATED",
            "expense",
            &expense_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn submit_expense(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        expense_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ExpenseResult, StoreError> {
        self.transition_expense(
            context,
            user,
            operation_id,
            expense_id,
            "SUBMIT",
            "DRAFT",
            "SUBMITTED",
            "expense.manage",
            None,
            now,
        )
    }

    pub fn decide_expense(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        expense_id: Uuid,
        approve: bool,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ExpenseResult, StoreError> {
        let (action, status) = if approve {
            ("APPROVE", "APPROVED")
        } else {
            ("REJECT", "REJECTED")
        };
        self.transition_expense(
            context,
            user,
            operation_id,
            expense_id,
            action,
            "SUBMITTED",
            status,
            "expense.approve",
            note,
            now,
        )
    }

    pub fn pay_expense(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        expense_id: Uuid,
        method: &str,
        reference: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ExpensePaymentResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(
            method,
            "CASH" | "CARD" | "BANK_TRANSFER" | "BENEFIT_PAY" | "CHEQUE" | "OTHER"
        ) {
            return Err(StoreError::Validation(
                "invalid expense payment method".into(),
            ));
        }
        let reference = reference.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(expense_id, method, reference))?);
        if let Some(result) =
            self.load_expense_operation(context.tenant_id, operation_id, "PAY", &digest)?
        {
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
            "expense.pay",
        )?;
        let amount: i64 = tx
            .query_row(
                "SELECT amount_fils FROM expenses WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='APPROVED'",
                params![expense_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Conflict("expense is not approved for payment".into()))?;
        let payment_id = Uuid::new_v4();
        tx.execute("INSERT INTO expense_payments(id,tenant_id,branch_id,expense_id,operation_id,device_id,user_id,method,amount_fils,reference,paid_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![payment_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), expense_id.to_string(), operation_id.to_string(), context.device_id.to_string(), user.to_string(), method, amount, reference, now.to_rfc3339()])?;
        let result = ExpensePaymentResult {
            payment_id,
            expense_id,
            status: "PAID".into(),
            amount: Money(amount),
        };
        Self::append_expense_event(
            &tx,
            context,
            user,
            expense_id,
            operation_id,
            "PAID",
            &result,
            now,
        )?;
        tx.execute("UPDATE expenses SET status='PAID',payment_method=?2,approved_by_user_id=COALESCE(approved_by_user_id,?3),paid_at=?4 WHERE id=?1", params![expense_id.to_string(), method, user.to_string(), now.to_rfc3339()])?;
        Self::record_expense_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PAY",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "EXPENSE_PAID",
            "expense",
            &expense_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn operating_profit_report(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        from_utc: &str,
        to_utc: &str,
    ) -> Result<OperatingProfitReport, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "expense.report",
        )?;
        if from_utc.trim().is_empty() || to_utc.trim().is_empty() || from_utc >= to_utc {
            return Err(StoreError::Validation("invalid report range".into()));
        }
        let tenant = context.tenant_id.to_string();
        let branch = context.branch_id.to_string();
        let sales = Self::sum_money_query(
            &self.conn,
            "SELECT subtotal_fils FROM sales WHERE tenant_id=?1 AND branch_id=?2 AND status='COMPLETED' AND completed_at>=?3 AND completed_at<?4",
            params![&tenant, &branch, from_utc, to_utc],
        )?;
        let refunds = Self::sum_money_query(
            &self.conn,
            "SELECT subtotal_fils FROM refunds WHERE tenant_id=?1 AND branch_id=?2 AND created_at>=?3 AND created_at<?4",
            params![&tenant, &branch, from_utc, to_utc],
        )?;
        let sale_cogs = Self::sum_money_query(
            &self.conn,
            "SELECT cogs_fils FROM sales WHERE tenant_id=?1 AND branch_id=?2 AND status='COMPLETED' AND completed_at>=?3 AND completed_at<?4",
            params![&tenant, &branch, from_utc, to_utc],
        )?;
        let mut refunded_cogs = Money::ZERO;
        let mut statement = self.conn.prepare("SELECT sl.unit_cost_fils,rl.quantity_milli FROM refund_lines rl JOIN refunds r ON r.id=rl.refund_id JOIN sale_lines sl ON sl.id=rl.sale_line_id WHERE r.tenant_id=?1 AND r.branch_id=?2 AND r.created_at>=?3 AND r.created_at<?4")?;
        let rows = statement.query_map(params![&tenant, &branch, from_utc, to_utc], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (unit_cost, quantity) = row?;
            refunded_cogs = refunded_cogs.checked_add(price_times_quantity(
                Money(unit_cost),
                QuantityMilli(quantity),
            )?)?;
        }
        let operating_expenses = Self::sum_money_query(
            &self.conn,
            "SELECT amount_fils FROM expenses WHERE tenant_id=?1 AND branch_id=?2 AND status='PAID' AND paid_at>=?3 AND paid_at<?4",
            params![&tenant, &branch, from_utc, to_utc],
        )?;
        let net_sales = sales.checked_sub(refunds)?;
        let cogs = sale_cogs.checked_sub(refunded_cogs)?;
        let gross_profit = net_sales.checked_sub(cogs)?;
        let operating_profit = gross_profit.checked_sub(operating_expenses)?;
        Ok(OperatingProfitReport {
            net_sales,
            cogs,
            gross_profit,
            operating_expenses,
            operating_profit,
        })
    }

    fn transition_expense(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        expense_id: Uuid,
        action: &str,
        expected_status: &str,
        next_status: &str,
        permission: &str,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ExpenseResult, StoreError> {
        self.validate_local_session(context, user)?;
        let note = note.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(expense_id, next_status, note))?);
        if let Some(result) =
            self.load_expense_operation(context.tenant_id, operation_id, action, &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(&tx, context.tenant_id, context.branch_id, user, permission)?;
        let values: (i64, i64) = tx
            .query_row(
                "SELECT amount_fils,tax_fils FROM expenses WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status=?4",
                params![expense_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), expected_status],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::Conflict("expense state transition is not allowed".into()))?;
        let result = ExpenseResult {
            expense_id,
            status: next_status.into(),
            amount: Money(values.0),
            tax: Money(values.1),
        };
        let evidence = serde_json::json!({"result": result, "note": note});
        Self::append_expense_event(
            &tx,
            context,
            user,
            expense_id,
            operation_id,
            next_status,
            &evidence,
            now,
        )?;
        tx.execute("UPDATE expenses SET status=?2,approved_by_user_id=CASE WHEN ?2 IN ('APPROVED','REJECTED') THEN ?3 ELSE approved_by_user_id END,approval_note=CASE WHEN ?2 IN ('APPROVED','REJECTED') THEN ?4 ELSE approval_note END WHERE id=?1", params![expense_id.to_string(), next_status, user.to_string(), note])?;
        Self::record_expense_operation(
            &tx,
            context.tenant_id,
            operation_id,
            action,
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            &format!("EXPENSE_{next_status}"),
            "expense",
            &expense_id.to_string(),
            &evidence.to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn append_expense_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        expense_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO expense_events(id,tenant_id,expense_id,event_type,operation_id,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), expense_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), serde_json::to_string(evidence)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn load_expense_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let global: Option<(String, String)> = self.conn.query_row("SELECT action,request_sha256 FROM idempotency_operations WHERE tenant_id=?1 AND operation_id=?2", params![tenant.to_string(), operation_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let expected = format!("EXPENSE:{action}");
        if let Some((stored_action, stored_digest)) = &global {
            if stored_action != &expected || stored_digest != digest {
                return Err(StoreError::Conflict(
                    "operation ID already bound to a different action or payload".into(),
                ));
            }
        }
        let row: Option<(String, String)> = self.conn.query_row("SELECT request_sha256,result_json FROM expense_operation_results WHERE tenant_id=?1 AND operation_id=?2 AND action=?3", params![tenant.to_string(), operation_id.to_string(), action], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        match row {
            Some((stored, json)) if stored == digest => Ok(Some(serde_json::from_str(&json)?)),
            Some(_) => Err(StoreError::Conflict(
                "expense operation ID reused with different payload".into(),
            )),
            None if global.is_some() => Err(StoreError::Conflict(
                "operation binding exists without expense result; recovery review required".into(),
            )),
            None => Ok(None),
        }
    }

    fn record_expense_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) VALUES(?1,?2,?3,?4,?5)", params![tenant.to_string(), operation_id.to_string(), format!("EXPENSE:{action}"), digest, now.to_rfc3339()])?;
        tx.execute("INSERT INTO expense_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)", params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()])?;
        Ok(())
    }

    fn sum_money_query<P: rusqlite::Params>(
        conn: &Connection,
        sql: &str,
        params: P,
    ) -> Result<Money, StoreError> {
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map(params, |row| row.get::<_, i64>(0))?;
        let mut total = Money::ZERO;
        for value in rows {
            total = total.checked_add(Money(value?))?;
        }
        Ok(total)
    }
}
