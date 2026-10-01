use super::*;

impl Store {
    pub fn create_recipe(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        output_product_id: ProductId,
        output_quantity: QuantityMilli,
        components: &[RecipeComponentInput],
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        if output_quantity.0 <= 0 || components.is_empty() {
            return Err(StoreError::Validation(
                "recipe output and components are required".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "production.manage",
        )?;
        Self::assert_product(&tx, context.tenant_id, output_product_id)?;
        let recipe_id = Uuid::new_v4();
        tx.execute("INSERT INTO recipes(id,tenant_id,output_product_id,output_qty_milli,active,version,created_at) VALUES(?1,?2,?3,?4,1,1,?5)", params![recipe_id.to_string(), context.tenant_id.to_string(), output_product_id.to_string(), output_quantity.0, now.to_rfc3339()])?;
        let mut seen = HashSet::new();
        for component in components {
            if component.quantity.0 <= 0
                || component.product_id == output_product_id
                || !seen.insert(component.product_id)
            {
                return Err(StoreError::Validation(
                    "invalid or duplicate recipe component".into(),
                ));
            }
            Self::assert_product(&tx, context.tenant_id, component.product_id)?;
            tx.execute("INSERT INTO recipe_components(recipe_id,component_product_id,quantity_milli) VALUES(?1,?2,?3)", params![recipe_id.to_string(), component.product_id.to_string(), component.quantity.0])?;
        }
        Self::append_audit(&tx, context.tenant_id, context.device_id, user, "RECIPE_CREATED", "recipe", &recipe_id.to_string(), &serde_json::json!({"output_product_id":output_product_id,"component_count":components.len()}).to_string(), now)?;
        tx.commit()?;
        Ok(recipe_id)
    }

    pub fn create_production_order(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        recipe_id: Uuid,
        centre_id: Uuid,
        planned_output: QuantityMilli,
        now: DateTime<Utc>,
    ) -> Result<ProductionResult, StoreError> {
        self.validate_local_session(context, user)?;
        if planned_output.0 <= 0 {
            return Err(StoreError::Validation(
                "planned output must be positive".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            recipe_id,
            centre_id,
            planned_output,
        ))?);
        if let Some(result) = self.load_store_operation(
            context.tenant_id,
            operation_id,
            "PRODUCTION_CREATE",
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
            "production.manage",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, centre_id)?;
        let recipe_ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM recipes WHERE id=?1 AND tenant_id=?2 AND active=1",
                params![recipe_id.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if recipe_ok.is_none() {
            return Err(StoreError::Authorization("recipe tenant mismatch"));
        }
        let production_order_id = Uuid::new_v4();
        tx.execute("INSERT INTO production_orders(id,tenant_id,branch_id,recipe_id,centre_id,status,planned_output_milli,operation_id,device_id,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,'PLANNED',?6,?7,?8,?9,?10)", params![production_order_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), recipe_id.to_string(), centre_id.to_string(), planned_output.0, operation_id.to_string(), context.device_id.to_string(), user.to_string(), now.to_rfc3339()])?;
        let result = ProductionResult {
            production_order_id,
            status: "PLANNED".into(),
            output_quantity: QuantityMilli(0),
            output_cost: Money::ZERO,
        };
        Self::append_production_event(
            &tx,
            context,
            user,
            production_order_id,
            operation_id,
            "CREATED",
            &result,
            now,
        )?;
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PRODUCTION_CREATE",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn complete_production(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        production_order_id: Uuid,
        operation_id: OperationId,
        actual_output: QuantityMilli,
        consumption: &[ProductionConsumptionInput],
        now: DateTime<Utc>,
    ) -> Result<ProductionResult, StoreError> {
        self.validate_local_session(context, user)?;
        if actual_output.0 <= 0 || consumption.is_empty() {
            return Err(StoreError::Validation(
                "production output and consumption are required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            production_order_id,
            actual_output,
            consumption,
        ))?);
        if let Some(result) = self.load_store_operation(
            context.tenant_id,
            operation_id,
            "PRODUCTION_COMPLETE",
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
            "production.manage",
        )?;
        let header: Option<(String, String, String, i64)> = tx.query_row("SELECT p.recipe_id,p.centre_id,r.output_product_id,r.output_qty_milli FROM production_orders p JOIN recipes r ON r.id=p.recipe_id WHERE p.id=?1 AND p.tenant_id=?2 AND p.branch_id=?3 AND p.status IN ('PLANNED','IN_PROGRESS')", params![production_order_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?;
        let (recipe, centre, output_product, recipe_output) = header.ok_or(
            StoreError::Conflict("production order is not completable".into()),
        )?;
        let centre_id = Uuid::parse_str(&centre)
            .map_err(|_| StoreError::Validation("invalid production centre".into()))?;
        let output_product_id = ProductId(
            Uuid::parse_str(&output_product)
                .map_err(|_| StoreError::Validation("invalid production output product".into()))?,
        );
        if recipe_output <= 0 {
            return Err(StoreError::Validation(
                "recipe output quantity is invalid".into(),
            ));
        }
        let expected_rows = {
            let mut statement = tx.prepare("SELECT component_product_id,quantity_milli FROM recipe_components WHERE recipe_id=?1")?;
            statement
                .query_map(params![recipe], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let expected_products = expected_rows
            .iter()
            .map(|(product, _)| product.as_str())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut output_cost = Money::ZERO;
        for input in consumption {
            let product_string = input.product_id.to_string();
            if input.quantity.0 <= 0
                || !seen.insert(input.product_id)
                || !expected_products.contains(product_string.as_str())
            {
                return Err(StoreError::Validation(
                    "production consumption does not match recipe".into(),
                ));
            }
            let valuation = Self::inventory_valuation_tx(
                &tx,
                context.tenant_id,
                context.branch_id,
                centre_id,
                input.product_id,
            )?;
            if valuation.quantity.0 < input.quantity.0 {
                return Err(StoreError::Conflict(
                    "insufficient component stock for production".into(),
                ));
            }
            let component_cost =
                price_times_quantity(valuation.weighted_average_cost, input.quantity)?;
            output_cost = output_cost.checked_add(component_cost)?;
            let expected_base = expected_rows
                .iter()
                .find(|(product, _)| product == &product_string)
                .map(|(_, quantity)| *quantity)
                .ok_or_else(|| {
                    StoreError::Validation("production component missing from recipe".into())
                })?;
            let expected = i64::try_from(
                (expected_base as i128)
                    .checked_mul(actual_output.0 as i128)
                    .ok_or(bhaipos_core::MoneyError::Overflow)?
                    / recipe_output as i128,
            )
            .map_err(|_| bhaipos_core::MoneyError::Overflow)?;
            tx.execute("INSERT INTO production_usage(id,production_order_id,product_id,expected_qty_milli,actual_qty_milli,lot_id,cost_fils) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![Uuid::new_v4().to_string(), production_order_id.to_string(), input.product_id.to_string(), expected, input.quantity.0, input.lot_id.map(|id| id.to_string()), component_cost.0])?;
            Self::append_inventory_effect(
                &tx,
                context,
                user,
                operation_id,
                centre_id,
                input.product_id,
                QuantityMilli(-input.quantity.0),
                valuation.weighted_average_cost,
                "PRODUCTION_CONSUMPTION",
                "PRODUCTION_ORDER",
                &production_order_id.to_string(),
                input.lot_id,
                now,
            )?;
        }
        if seen.len() != expected_products.len() {
            return Err(StoreError::Validation(
                "every recipe component requires actual consumption".into(),
            ));
        }
        let unit_cost = {
            let numerator = (output_cost.0 as i128)
                .checked_mul(1000)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            Money(
                i64::try_from((numerator + actual_output.0 as i128 / 2) / actual_output.0 as i128)
                    .map_err(|_| bhaipos_core::MoneyError::Overflow)?,
            )
        };
        Self::append_inventory_effect(
            &tx,
            context,
            user,
            operation_id,
            centre_id,
            output_product_id,
            actual_output,
            unit_cost,
            "PRODUCTION_OUTPUT",
            "PRODUCTION_ORDER",
            &production_order_id.to_string(),
            None,
            now,
        )?;
        tx.execute("UPDATE production_orders SET status='COMPLETED',actual_output_milli=?2,completed_at=?3 WHERE id=?1", params![production_order_id.to_string(), actual_output.0, now.to_rfc3339()])?;
        let result = ProductionResult {
            production_order_id,
            status: "COMPLETED".into(),
            output_quantity: actual_output,
            output_cost,
        };
        Self::append_production_event(
            &tx,
            context,
            user,
            production_order_id,
            operation_id,
            "COMPLETED",
            &result,
            now,
        )?;
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "PRODUCTION_COMPLETE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "PRODUCTION_COMPLETED",
            "production_order",
            &production_order_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn append_production_event<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        production_order_id: Uuid,
        operation_id: OperationId,
        event_type: &str,
        evidence: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO production_events(id,tenant_id,production_order_id,event_type,operation_id,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), production_order_id.to_string(), event_type, operation_id.to_string(), context.device_id.to_string(), user.to_string(), serde_json::to_string(evidence)?, now.to_rfc3339()])?;
        Ok(())
    }
}
