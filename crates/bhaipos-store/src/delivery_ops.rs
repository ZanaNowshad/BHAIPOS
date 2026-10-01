#![allow(
    clippy::too_many_arguments,
    reason = "trusted delivery commands keep authority, evidence, and event time explicit"
)]

use super::*;

impl Store {
    pub fn create_delivery_worker(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        worker_id: Uuid,
        name: &str,
        phone: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Validation("delivery worker name is required".into()));
        }
        let phone = phone.map(Self::normalize_phone).transpose()?;
        let digest = sha256_hex(&serde_json::to_vec(&(worker_id, name, &phone))?);
        if let Some(result) = self.load_delivery_operation(
            context.tenant_id,
            operation_id,
            "WORKER_CREATE",
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
            "delivery.manage",
        )?;
        tx.execute(
            "INSERT INTO delivery_workers(id,tenant_id,name,phone_e164,branch_id,active) VALUES(?1,?2,?3,?4,?5,1)",
            params![worker_id.to_string(), context.tenant_id.to_string(), name, phone, context.branch_id.to_string()],
        )?;
        Self::record_delivery_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "WORKER_CREATE",
            &digest,
            &worker_id,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "DELIVERY_WORKER_CREATED",
            "delivery_worker",
            &worker_id.to_string(),
            &serde_json::json!({"name": name}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(worker_id)
    }

    pub fn create_delivery_order(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        delivery_id: Uuid,
        sale_id: Option<SaleId>,
        customer_id: Option<Uuid>,
        phone: Option<&str>,
        amount_due: Money,
        notes: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<DeliveryResult, StoreError> {
        self.validate_local_session(context, user)?;
        if amount_due.0 <= 0 {
            return Err(StoreError::Validation(
                "delivery amount due must be positive".into(),
            ));
        }
        let phone = phone.map(Self::normalize_phone).transpose()?;
        let notes = notes.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(
            delivery_id,
            sale_id,
            customer_id,
            &phone,
            amount_due,
            notes,
        ))?);
        if let Some(result) =
            self.load_delivery_operation(context.tenant_id, operation_id, "CREATE", &digest)?
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
            "delivery.manage",
        )?;
        tx.execute(
            "INSERT INTO delivery_orders(id,tenant_id,branch_id,sale_id,customer_id,phone_e164,amount_due_fils,payment_state,status,notes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'DUE','PENDING',?8,?9)",
            params![delivery_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), sale_id.map(|id| id.to_string()), customer_id.map(|id| id.to_string()), phone, amount_due.0, notes, now.to_rfc3339()],
        )?;
        let result = DeliveryResult {
            delivery_id,
            status: "PENDING".into(),
            payment_state: "DUE".into(),
            amount_due,
            assigned_worker_id: None,
        };
        Self::append_delivery_state_event(
            &tx,
            context,
            user,
            delivery_id,
            operation_id,
            "CREATED",
            None,
            &result,
            now,
        )?;
        Self::record_delivery_operation(
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
            "DELIVERY_CREATED",
            "delivery_order",
            &delivery_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn transition_delivery(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        delivery_id: Uuid,
        new_status: &str,
        worker_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<DeliveryResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(
            new_status,
            "PREPARING" | "READY" | "DISPATCHED" | "DELIVERED" | "CANCELLED" | "RETURNED"
        ) || (new_status == "DISPATCHED") != worker_id.is_some()
        {
            return Err(StoreError::Validation(
                "invalid delivery transition or worker assignment".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(delivery_id, new_status, worker_id))?);
        if let Some(result) = self.load_delivery_operation(
            context.tenant_id,
            operation_id,
            "TRANSITION",
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
            "delivery.dispatch",
        )?;
        let (current_status, payment_state, amount_due, assigned_worker):
            (String, String, i64, Option<String>) = tx
            .query_row(
                "SELECT status,payment_state,amount_due_fils,assigned_worker_id FROM delivery_orders WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
                params![delivery_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or(StoreError::NotFound("delivery order"))?;
        let valid = matches!(
            (current_status.as_str(), new_status),
            ("PENDING", "PREPARING")
                | ("PENDING", "CANCELLED")
                | ("PREPARING", "READY")
                | ("PREPARING", "CANCELLED")
                | ("READY", "DISPATCHED")
                | ("READY", "CANCELLED")
                | ("DISPATCHED", "DELIVERED")
                | ("DISPATCHED", "RETURNED")
        );
        if !valid {
            return Err(StoreError::Conflict("illegal delivery state transition".into()));
        }
        Self::append_delivery_state_event(
            &tx,
            context,
            user,
            delivery_id,
            operation_id,
            new_status,
            worker_id,
            &serde_json::json!({"from": current_status, "to": new_status}),
            now,
        )?;
        if let Some(worker) = worker_id {
            tx.execute(
                "UPDATE delivery_orders SET assigned_worker_id=?2,status=?3,dispatched_at=?4 WHERE id=?1",
                params![delivery_id.to_string(), worker.to_string(), new_status, now.to_rfc3339()],
            )?;
        } else if new_status == "DELIVERED" {
            tx.execute(
                "UPDATE delivery_orders SET status=?2,delivered_at=?3 WHERE id=?1",
                params![delivery_id.to_string(), new_status, now.to_rfc3339()],
            )?;
        } else {
            tx.execute(
                "UPDATE delivery_orders SET status=?2 WHERE id=?1",
                params![delivery_id.to_string(), new_status],
            )?;
        }
        let assigned_worker_id = worker_id
            .or_else(|| assigned_worker.and_then(|value| Uuid::parse_str(&value).ok()));
        let result = DeliveryResult {
            delivery_id,
            status: new_status.into(),
            payment_state,
            amount_due: Money(amount_due),
            assigned_worker_id,
        };
        Self::record_delivery_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "TRANSITION",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "DELIVERY_STATE_CHANGED",
            "delivery_order",
            &delivery_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn collect_delivery_payment(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        delivery_id: Uuid,
        method: &str,
        amount: Money,
        reference: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<DeliveryCollectionResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(method, "CASH" | "CARD" | "BANK_TRANSFER" | "BENEFIT_PAY" | "OTHER")
            || amount.0 <= 0
        {
            return Err(StoreError::Validation("invalid delivery collection".into()));
        }
        let reference = reference.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(
            delivery_id,
            method,
            amount,
            reference,
        ))?);
        if let Some(result) = self.load_delivery_operation(
            context.tenant_id,
            operation_id,
            "COLLECT",
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
            "delivery.collect",
        )?;
        let worker: String = tx
            .query_row(
                "SELECT assigned_worker_id FROM delivery_orders WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='DISPATCHED' AND payment_state='DUE' AND amount_due_fils=?4",
                params![delivery_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), amount.0],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Conflict("delivery is not collectible for this amount".into()))?;
        let collection_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO delivery_collections(id,tenant_id,branch_id,delivery_id,worker_id,operation_id,device_id,user_id,method,amount_fils,reference,collected_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![collection_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), delivery_id.to_string(), worker, operation_id.to_string(), context.device_id.to_string(), user.to_string(), method, amount.0, reference, now.to_rfc3339()],
        )?;
        tx.execute(
            "UPDATE delivery_orders SET payment_state='PAID' WHERE id=?1",
            params![delivery_id.to_string()],
        )?;
        let result = DeliveryCollectionResult {
            collection_id,
            delivery_id,
            payment_state: "PAID".into(),
            method: method.into(),
            amount,
        };
        Self::record_delivery_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "COLLECT",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "DELIVERY_PAYMENT_COLLECTED",
            "delivery_collection",
            &collection_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn open_courier_cash(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        worker_id: Uuid,
    ) -> Result<Money, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "delivery.settle",
        )?;
        let amount = self.conn.query_row(
            "SELECT COALESCE(SUM(c.amount_fils),0) FROM delivery_collections c WHERE c.tenant_id=?1 AND c.branch_id=?2 AND c.worker_id=?3 AND c.method='CASH' AND NOT EXISTS (SELECT 1 FROM delivery_cash_settlement_allocations a WHERE a.collection_id=c.id)",
            params![context.tenant_id.to_string(), context.branch_id.to_string(), worker_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(Money(amount))
    }

    pub fn settle_courier_cash(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        worker_id: Uuid,
        returned_cash: Money,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<CourierCashSettlementResult, StoreError> {
        self.validate_local_session(context, user)?;
        if returned_cash.0 < 0 {
            return Err(StoreError::Validation(
                "returned courier cash cannot be negative".into(),
            ));
        }
        let note = note.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(worker_id, returned_cash, note))?);
        if let Some(result) = self.load_delivery_operation(
            context.tenant_id,
            operation_id,
            "SETTLE_CASH",
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
            "delivery.settle",
        )?;
        let collections = {
            let mut statement = tx.prepare(
                "SELECT c.id,c.amount_fils FROM delivery_collections c WHERE c.tenant_id=?1 AND c.branch_id=?2 AND c.worker_id=?3 AND c.method='CASH' AND NOT EXISTS (SELECT 1 FROM delivery_cash_settlement_allocations a WHERE a.collection_id=c.id) ORDER BY c.collected_at,c.id",
            )?;
            let rows = statement.query_map(
                params![context.tenant_id.to_string(), context.branch_id.to_string(), worker_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let expected = collections.iter().try_fold(0_i64, |total, (_, amount)| {
            total.checked_add(*amount).ok_or(bhaipos_core::MoneyError::Overflow)
        })?;
        if expected <= 0 {
            return Err(StoreError::Conflict("courier has no unsettled cash".into()));
        }
        let variance = returned_cash.checked_sub(Money(expected))?;
        if variance.0 != 0 && note.is_none() {
            return Err(StoreError::Validation(
                "courier cash discrepancy note is required".into(),
            ));
        }
        let status = if variance.0 == 0 {
            "SETTLED"
        } else {
            "DISCREPANCY"
        };
        let settlement_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO delivery_cash_settlements(id,tenant_id,branch_id,worker_id,operation_id,device_id,user_id,expected_cash_fils,returned_cash_fils,variance_fils,status,note,settled_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![settlement_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), worker_id.to_string(), operation_id.to_string(), context.device_id.to_string(), user.to_string(), expected, returned_cash.0, variance.0, status, note, now.to_rfc3339()],
        )?;
        for (collection_id, amount) in collections {
            tx.execute(
                "INSERT INTO delivery_cash_settlement_allocations(settlement_id,collection_id,tenant_id,amount_fils) VALUES(?1,?2,?3,?4)",
                params![settlement_id.to_string(), collection_id, context.tenant_id.to_string(), amount],
            )?;
        }
        let result = CourierCashSettlementResult {
            settlement_id,
            worker_id,
            expected_cash: Money(expected),
            returned_cash,
            variance,
            status: status.into(),
        };
        Self::record_delivery_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "SETTLE_CASH",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "COURIER_CASH_SETTLED",
            "delivery_cash_settlement",
            &settlement_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn append_delivery_state_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        delivery_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        worker_id: Option<Uuid>,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO delivery_state_events(id,tenant_id,delivery_id,event_type,operation_id,device_id,user_id,worker_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), delivery_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), worker_id.map(|id| id.to_string()), serde_json::to_string(evidence)?, now.to_rfc3339()],
        )?;
        Ok(())
    }

    fn load_delivery_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let stored: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT action,request_sha256,result_json FROM delivery_operation_results WHERE tenant_id=?1 AND operation_id=?2",
                params![tenant.to_string(), operation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match stored {
            None => Ok(None),
            Some((stored_action, stored_digest, json))
                if stored_action == action && stored_digest == digest =>
            {
                Ok(Some(serde_json::from_str(&json)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "delivery operation ID reused with different action or payload".into(),
            )),
        }
    }

    fn record_delivery_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO delivery_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()],
        )?;
        Ok(())
    }
}
