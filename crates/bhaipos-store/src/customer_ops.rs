use super::*;

impl Store {
    pub fn create_customer(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        customer_id: Uuid,
        name: &str,
        phone: Option<&str>,
        whatsapp: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        let phone = phone.map(Self::normalize_phone).transpose()?;
        let whatsapp = whatsapp.map(Self::normalize_phone).transpose()?;
        let digest = sha256_hex(&serde_json::to_vec(&(
            customer_id,
            name.trim(),
            &phone,
            &whatsapp,
        ))?);
        if let Some(result) =
            self.load_store_operation(context.tenant_id, operation_id, "CUSTOMER_CREATE", &digest)?
        {
            return Ok(result);
        }
        if name.trim().is_empty() {
            return Err(StoreError::Validation("customer name is required".into()));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "customer.manage",
        )?;
        let customer_no = format!("C-{}", &customer_id.simple().to_string()[..10]);
        tx.execute("INSERT INTO customers(id,tenant_id,customer_no,name,phone_e164,whatsapp_e164,active,created_at) VALUES(?1,?2,?3,?4,?5,?6,1,?7)", params![customer_id.to_string(), context.tenant_id.to_string(), customer_no, name.trim(), phone, whatsapp, now.to_rfc3339()])?;
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CUSTOMER_CREATE",
            &digest,
            &customer_id,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "CUSTOMER_CREATED",
            "customer",
            &customer_id.to_string(),
            &serde_json::json!({"name":name.trim()}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(customer_id)
    }

    pub fn associate_cart_customer(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        cart_id: CartId,
        customer_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        let changed = self.conn.execute("UPDATE carts SET customer_id=?6,version=version+1,updated_at=?7 WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4 AND cashier_user_id=?5 AND status IN ('ACTIVE','HELD') AND EXISTS(SELECT 1 FROM customers c WHERE c.id=?6 AND c.tenant_id=?2 AND c.active=1)", params![cart_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), context.device_id.to_string(), user.to_string(), customer_id.to_string(), now.to_rfc3339()])?;
        if changed != 1 {
            return Err(StoreError::Authorization("cart or customer scope mismatch"));
        }
        Ok(())
    }

    pub fn set_customer_credit_account(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        customer_id: Uuid,
        limit: Money,
        terms_days: Option<i64>,
        status: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "customer.credit.manage",
        )?;
        if limit.0 < 0
            || terms_days.is_some_and(|days| days < 0)
            || !matches!(status, "ACTIVE" | "SUSPENDED" | "CLOSED")
        {
            return Err(StoreError::Validation(
                "invalid customer credit policy".into(),
            ));
        }
        let customer_ok: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM customers WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![customer_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if customer_ok.is_none() {
            return Err(StoreError::Authorization("customer tenant mismatch"));
        }
        self.conn.execute("INSERT INTO customer_credit_accounts(tenant_id,customer_id,credit_limit_fils,payment_terms_days,status,opened_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(customer_id) DO UPDATE SET credit_limit_fils=excluded.credit_limit_fils,payment_terms_days=excluded.payment_terms_days,status=excluded.status", params![context.tenant_id.to_string(), customer_id.to_string(), limit.0, terms_days, status, now.to_rfc3339()])?;
        Ok(())
    }

    pub fn record_loyalty_event(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        customer_id: Uuid,
        event_type: &str,
        points_delta: i64,
        source_type: &str,
        source_id: Option<&str>,
        expires_at: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<LoyaltyEventResult, StoreError> {
        self.validate_local_session(context, user)?;
        if points_delta == 0
            || !matches!(
                event_type,
                "EARN" | "REDEEM" | "REFUND" | "REVERSAL" | "ADJUSTMENT" | "EXPIRY"
            )
        {
            return Err(StoreError::Validation("invalid loyalty event".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            customer_id,
            event_type,
            points_delta,
            source_type,
            source_id,
            expires_at,
        ))?);
        if let Some(result) =
            self.load_store_operation(context.tenant_id, operation_id, "LOYALTY_EVENT", &digest)?
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
            "loyalty.adjust",
        )?;
        let current: i64 = tx.query_row("SELECT COALESCE(SUM(points_delta),0) FROM loyalty_ledger WHERE tenant_id=?1 AND customer_id=?2", params![context.tenant_id.to_string(), customer_id.to_string()], |row| row.get(0))?;
        let next = current
            .checked_add(points_delta)
            .ok_or_else(|| StoreError::Validation("loyalty balance overflow".into()))?;
        if next < 0 {
            return Err(StoreError::Conflict(
                "loyalty event exceeds points balance".into(),
            ));
        }
        let loyalty_event_id = Uuid::new_v4();
        tx.execute("INSERT INTO loyalty_ledger(id,tenant_id,customer_id,event_type,points_delta,source_type,source_id,expires_at,operation_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![loyalty_event_id.to_string(), context.tenant_id.to_string(), customer_id.to_string(), event_type, points_delta, source_type, source_id, expires_at, operation_id.to_string(), now.to_rfc3339()])?;
        let result = LoyaltyEventResult {
            loyalty_event_id,
            points_balance: next,
        };
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "LOYALTY_EVENT",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "LOYALTY_EVENT_POSTED",
            "loyalty_event",
            &loyalty_event_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn receive_customer_credit_payment(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        customer_id: Uuid,
        method: &str,
        amount: Money,
        reference: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<CustomerCreditPaymentResult, StoreError> {
        self.validate_local_session(context, user)?;
        if amount.0 <= 0
            || !matches!(
                method,
                "CASH" | "CARD" | "BANK_TRANSFER" | "BENEFIT_PAY" | "CHEQUE" | "OTHER"
            )
        {
            return Err(StoreError::Validation(
                "invalid customer credit payment".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            customer_id,
            method,
            amount,
            reference.map(str::trim),
        ))?);
        if let Some(result) = self.load_store_operation(
            context.tenant_id,
            operation_id,
            "CUSTOMER_CREDIT_PAYMENT",
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
            "customer.credit.collect",
        )?;
        let outstanding: i64 = tx.query_row("SELECT COALESCE(SUM(debit_fils-credit_fils),0) FROM customer_credit_ledger WHERE tenant_id=?1 AND customer_id=?2", params![context.tenant_id.to_string(), customer_id.to_string()], |row| row.get(0))?;
        if amount.0 > outstanding {
            return Err(StoreError::Conflict(
                "customer credit payment exceeds outstanding balance".into(),
            ));
        }
        let payment_id = Uuid::new_v4();
        tx.execute("INSERT INTO customer_credit_payments(id,tenant_id,customer_id,branch_id,operation_id,device_id,user_id,method,amount_fils,reference,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![payment_id.to_string(), context.tenant_id.to_string(), customer_id.to_string(), context.branch_id.to_string(), operation_id.to_string(), context.device_id.to_string(), user.to_string(), method, amount.0, reference.map(str::trim), now.to_rfc3339()])?;
        tx.execute("INSERT INTO customer_credit_ledger(id,tenant_id,customer_id,event_type,source_type,source_id,debit_fils,credit_fils,operation_id,created_at) VALUES(?1,?2,?3,'PAYMENT','CUSTOMER_CREDIT_PAYMENT',?4,0,?5,?6,?7)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), customer_id.to_string(), payment_id.to_string(), amount.0, operation_id.to_string(), now.to_rfc3339()])?;
        let result = CustomerCreditPaymentResult {
            payment_id,
            amount,
            outstanding_balance: Money(outstanding).checked_sub(amount)?,
        };
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CUSTOMER_CREDIT_PAYMENT",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "CUSTOMER_CREDIT_PAYMENT_POSTED",
            "customer_credit_payment",
            &payment_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn customer_credit_balance(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        customer_id: Uuid,
        as_of: &str,
    ) -> Result<CustomerCreditBalance, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "customer.credit.view",
        )?;
        let limit: i64 = self.conn.query_row("SELECT credit_limit_fils FROM customer_credit_accounts WHERE tenant_id=?1 AND customer_id=?2", params![context.tenant_id.to_string(), customer_id.to_string()], |row| row.get(0)).optional()?.ok_or(StoreError::NotFound("customer credit account"))?;
        let outstanding: i64 = self.conn.query_row("SELECT COALESCE(SUM(debit_fils-credit_fils),0) FROM customer_credit_ledger WHERE tenant_id=?1 AND customer_id=?2", params![context.tenant_id.to_string(), customer_id.to_string()], |row| row.get(0))?;
        let overdue: i64 = self.conn.query_row("SELECT COALESCE(SUM(debit_fils-credit_fils),0) FROM customer_credit_ledger WHERE tenant_id=?1 AND customer_id=?2 AND due_date IS NOT NULL AND due_date<?3", params![context.tenant_id.to_string(), customer_id.to_string(), as_of], |row| row.get(0))?;
        Ok(CustomerCreditBalance {
            credit_limit: Money(limit),
            outstanding: Money(outstanding),
            available: Money(limit).checked_sub(Money(outstanding))?,
            overdue: Money(overdue.max(0)),
        })
    }

    fn normalize_phone(input: &str) -> Result<String, StoreError> {
        let trimmed = input.trim();
        let digits = trimmed
            .chars()
            .filter(|value| value.is_ascii_digit())
            .collect::<String>();
        let normalized = if trimmed.starts_with('+') {
            format!("+{digits}")
        } else if digits.len() == 8 {
            format!("+973{digits}")
        } else if digits.starts_with("00") {
            format!("+{}", &digits[2..])
        } else {
            format!("+{digits}")
        };
        if normalized.len() < 9 || normalized.len() > 16 {
            return Err(StoreError::Validation("invalid phone number".into()));
        }
        Ok(normalized)
    }

    pub(super) fn load_store_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let global: Option<(String, String)> = self.conn.query_row("SELECT action,request_sha256 FROM idempotency_operations WHERE tenant_id=?1 AND operation_id=?2", params![tenant.to_string(), operation_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let expected = format!("STORE:{action}");
        if let Some((stored_action, stored_digest)) = &global {
            if stored_action != &expected || stored_digest != digest {
                return Err(StoreError::Conflict(
                    "operation ID already bound to a different action or payload".into(),
                ));
            }
        }
        let row: Option<(String, String)> = self.conn.query_row("SELECT request_sha256,result_json FROM store_operation_results WHERE tenant_id=?1 AND operation_id=?2 AND action=?3", params![tenant.to_string(), operation_id.to_string(), action], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        match row {
            Some((stored, json)) if stored == digest => Ok(Some(serde_json::from_str(&json)?)),
            Some(_) => Err(StoreError::Conflict(
                "store operation ID reused with different payload".into(),
            )),
            None if global.is_some() => Err(StoreError::Conflict(
                "operation binding exists without store result; recovery review required".into(),
            )),
            None => Ok(None),
        }
    }

    pub(super) fn record_store_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) VALUES(?1,?2,?3,?4,?5)", params![tenant.to_string(), operation_id.to_string(), format!("STORE:{action}"), digest, now.to_rfc3339()])?;
        tx.execute("INSERT INTO store_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)", params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()])?;
        Ok(())
    }
}
