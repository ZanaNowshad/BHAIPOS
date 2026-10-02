#![allow(
    clippy::too_many_arguments,
    reason = "attendance commands keep authority, employee identity, retry identity, and event time explicit"
)]

use super::*;

impl Store {
    pub fn create_employee(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        employee_id: Uuid,
        employee_no: &str,
        name: &str,
        job_title: Option<&str>,
        phone: Option<&str>,
        hire_date: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<EmployeeResult, StoreError> {
        self.validate_local_session(context, user)?;
        let employee_no = employee_no.trim();
        let name = name.trim();
        if employee_no.is_empty() || name.is_empty() {
            return Err(StoreError::Validation(
                "employee number and name are required".into(),
            ));
        }
        let job_title = job_title.map(str::trim).filter(|value| !value.is_empty());
        let phone = phone.map(Self::normalize_phone).transpose()?;
        let hire_date = hire_date.map(str::trim).filter(|value| !value.is_empty());
        let digest = sha256_hex(&serde_json::to_vec(&(
            employee_id,
            employee_no,
            name,
            job_title,
            &phone,
            hire_date,
            context.branch_id,
        ))?);
        if let Some(result) = self.load_employee_operation(
            context.tenant_id,
            operation_id,
            "CREATE",
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
            "employee.manage",
        )?;
        tx.execute(
            "INSERT INTO employees(id,tenant_id,employee_no,name,job_title,phone_e164,hire_date,status) VALUES(?1,?2,?3,?4,?5,?6,?7,'ACTIVE')",
            params![employee_id.to_string(), context.tenant_id.to_string(), employee_no, name, job_title, phone, hire_date],
        )?;
        tx.execute(
            "INSERT INTO employee_branches(employee_id,branch_id) VALUES(?1,?2)",
            params![employee_id.to_string(), context.branch_id.to_string()],
        )?;
        let result = EmployeeResult {
            employee_id,
            employee_no: employee_no.into(),
            name: name.into(),
            status: "ACTIVE".into(),
        };
        Self::record_scoped_operation(
            &tx,
            "employee_operation_results",
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
            "EMPLOYEE_CREATED",
            "employee",
            &employee_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn record_attendance_event(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        employee_id: Uuid,
        event_type: &str,
        occurred_at: DateTime<Utc>,
        note: Option<&str>,
    ) -> Result<AttendanceResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(
            event_type,
            "CLOCK_IN" | "BREAK_START" | "BREAK_END" | "CLOCK_OUT" | "MISSING_CLOCK_OUT"
        ) {
            return Err(StoreError::Validation("invalid attendance event".into()));
        }
        let note = note.map(str::trim).filter(|value| !value.is_empty());
        if event_type == "MISSING_CLOCK_OUT" && note.is_none() {
            return Err(StoreError::Validation(
                "missing clock-out resolution requires a note".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            employee_id,
            event_type,
            occurred_at,
            note,
        ))?);
        if let Some(result) = self.load_attendance_operation(
            context.tenant_id,
            operation_id,
            "EVENT",
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
            if event_type == "MISSING_CLOCK_OUT" {
                "attendance.manage"
            } else {
                "attendance.clock"
            },
        )?;
        let result = if event_type == "CLOCK_IN" {
            let employee_exists: Option<i32> = tx
                .query_row(
                    "SELECT 1 FROM employees e JOIN employee_branches eb ON eb.employee_id=e.id WHERE e.id=?1 AND e.tenant_id=?2 AND e.status='ACTIVE' AND eb.branch_id=?3",
                    params![employee_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if employee_exists.is_none() {
                return Err(StoreError::Authorization(
                    "employee is not active at this branch",
                ));
            }
            let active: Option<i32> = tx
                .query_row(
                    "SELECT 1 FROM attendance_sessions WHERE tenant_id=?1 AND employee_id=?2 AND state IN ('CLOCKED_IN','ON_BREAK')",
                    params![context.tenant_id.to_string(), employee_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if active.is_some() {
                return Err(StoreError::Conflict("employee is already clocked in".into()));
            }
            let session_id = Uuid::new_v4();
            tx.execute(
                "INSERT INTO attendance_sessions(id,tenant_id,employee_id,branch_id,state,clocked_in_at,break_seconds,created_device_id,created_by_user_id,last_operation_id,updated_at) VALUES(?1,?2,?3,?4,'CLOCKED_IN',?5,0,?6,?7,?8,?5)",
                params![session_id.to_string(), context.tenant_id.to_string(), employee_id.to_string(), context.branch_id.to_string(), occurred_at.to_rfc3339(), context.device_id.to_string(), user.to_string(), operation_id.to_string()],
            )?;
            Self::append_attendance_event(
                &tx,
                context,
                user,
                operation_id,
                session_id,
                employee_id,
                event_type,
                None,
                "CLOCKED_IN",
                occurred_at,
                note,
            )?;
            AttendanceResult {
                session_id,
                employee_id,
                state: "CLOCKED_IN".into(),
                break_seconds: 0,
                worked_seconds: None,
            }
        } else {
            let row: Option<(String, String, String, Option<String>, i64)> = tx
                .query_row(
                    "SELECT id,state,clocked_in_at,active_break_started_at,break_seconds FROM attendance_sessions WHERE tenant_id=?1 AND employee_id=?2 AND branch_id=?3 AND state IN ('CLOCKED_IN','ON_BREAK')",
                    params![context.tenant_id.to_string(), employee_id.to_string(), context.branch_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .optional()?;
            let (session, previous_state, clocked_in, break_started, mut break_seconds) = row
                .ok_or_else(|| StoreError::Conflict("employee has no active attendance session".into()))?;
            let session_id = Uuid::parse_str(&session)
                .map_err(|_| StoreError::Validation("invalid attendance session identity".into()))?;
            let last_at: String = tx.query_row(
                "SELECT occurred_at FROM attendance_session_events WHERE session_id=?1 ORDER BY occurred_at DESC,id DESC LIMIT 1",
                params![&session],
                |row| row.get(0),
            )?;
            let last_at = DateTime::parse_from_rfc3339(&last_at)
                .map_err(|_| StoreError::Validation("invalid stored attendance time".into()))?
                .with_timezone(&Utc);
            if occurred_at <= last_at {
                return Err(StoreError::Conflict(
                    "attendance event time must advance".into(),
                ));
            }
            let new_state = match (previous_state.as_str(), event_type) {
                ("CLOCKED_IN", "BREAK_START") => "ON_BREAK",
                ("ON_BREAK", "BREAK_END") => "CLOCKED_IN",
                ("CLOCKED_IN" | "ON_BREAK", "CLOCK_OUT") => "CLOCKED_OUT",
                ("CLOCKED_IN" | "ON_BREAK", "MISSING_CLOCK_OUT") => "MISSING_CLOCK_OUT",
                _ => return Err(StoreError::Conflict("illegal attendance transition".into())),
            };
            if previous_state == "ON_BREAK" && matches!(event_type, "BREAK_END" | "CLOCK_OUT" | "MISSING_CLOCK_OUT") {
                let started = break_started
                    .as_deref()
                    .ok_or_else(|| StoreError::Conflict("active break has no start time".into()))?;
                let started = DateTime::parse_from_rfc3339(started)
                    .map_err(|_| StoreError::Validation("invalid stored break time".into()))?
                    .with_timezone(&Utc);
                break_seconds = break_seconds
                    .checked_add((occurred_at - started).num_seconds())
                    .ok_or(bhaipos_core::MoneyError::Overflow)?;
            }
            let worked_seconds = if matches!(new_state, "CLOCKED_OUT" | "MISSING_CLOCK_OUT") {
                let clocked_in = DateTime::parse_from_rfc3339(&clocked_in)
                    .map_err(|_| StoreError::Validation("invalid stored clock-in time".into()))?
                    .with_timezone(&Utc);
                let elapsed = (occurred_at - clocked_in).num_seconds();
                let worked = elapsed
                    .checked_sub(break_seconds)
                    .ok_or(bhaipos_core::MoneyError::Overflow)?;
                if worked < 0 {
                    return Err(StoreError::Conflict("attendance duration is negative".into()));
                }
                Some(worked)
            } else {
                None
            };
            Self::append_attendance_event(
                &tx,
                context,
                user,
                operation_id,
                session_id,
                employee_id,
                event_type,
                Some(&previous_state),
                new_state,
                occurred_at,
                note,
            )?;
            let active_break = if new_state == "ON_BREAK" {
                Some(occurred_at.to_rfc3339())
            } else {
                None
            };
            let clocked_out = if worked_seconds.is_some() {
                Some(occurred_at.to_rfc3339())
            } else {
                None
            };
            tx.execute(
                "UPDATE attendance_sessions SET state=?2,clocked_out_at=?3,active_break_started_at=?4,break_seconds=?5,worked_seconds=?6,last_operation_id=?7,updated_at=?8 WHERE id=?1",
                params![session, new_state, clocked_out, active_break, break_seconds, worked_seconds, operation_id.to_string(), occurred_at.to_rfc3339()],
            )?;
            AttendanceResult {
                session_id,
                employee_id,
                state: new_state.into(),
                break_seconds,
                worked_seconds,
            }
        };
        Self::record_scoped_operation(
            &tx,
            "attendance_operation_results",
            context.tenant_id,
            operation_id,
            "EVENT",
            &digest,
            &result,
            occurred_at,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "ATTENDANCE_EVENT_RECORDED",
            "attendance_session",
            &result.session_id.to_string(),
            &serde_json::json!({"event_type":event_type,"result":result}).to_string(),
            occurred_at,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn attendance_report(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        employee_id: Uuid,
        from_utc: &str,
        to_utc: &str,
    ) -> Result<AttendanceReport, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "attendance.view",
        )?;
        if from_utc.trim().is_empty() || to_utc.trim().is_empty() || from_utc >= to_utc {
            return Err(StoreError::Validation("invalid attendance report range".into()));
        }
        let (completed, worked, breaks, missing) = self.conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(worked_seconds),0),COALESCE(SUM(break_seconds),0),COALESCE(SUM(CASE WHEN state='MISSING_CLOCK_OUT' THEN 1 ELSE 0 END),0) FROM attendance_sessions WHERE tenant_id=?1 AND branch_id=?2 AND employee_id=?3 AND state IN ('CLOCKED_OUT','MISSING_CLOCK_OUT') AND clocked_in_at>=?4 AND clocked_in_at<?5",
            params![context.tenant_id.to_string(), context.branch_id.to_string(), employee_id.to_string(), from_utc, to_utc],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        Ok(AttendanceReport {
            employee_id,
            completed_sessions: completed,
            worked_seconds: worked,
            break_seconds: breaks,
            missing_clock_out_sessions: missing,
        })
    }

    fn append_attendance_event(
        tx: &Transaction<'_>, context: LocalTerminalContext, user: UserId,
        operation_id: OperationId, session_id: Uuid, employee_id: Uuid,
        event_type: &str, previous_state: Option<&str>, new_state: &str,
        occurred_at: DateTime<Utc>, note: Option<&str>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO attendance_session_events(id,tenant_id,session_id,employee_id,branch_id,operation_id,event_type,previous_state,new_state,occurred_at,device_id,entered_by_user_id,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?10)",
            params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), session_id.to_string(), employee_id.to_string(), context.branch_id.to_string(), operation_id.to_string(), event_type, previous_state, new_state, occurred_at.to_rfc3339(), context.device_id.to_string(), user.to_string(), note],
        )?;
        Ok(())
    }

    fn load_employee_operation<T: DeserializeOwned>(&self, tenant: TenantId, operation_id: OperationId, action: &str, digest: &str) -> Result<Option<T>, StoreError> {
        Self::load_scoped_operation(&self.conn, "employee_operation_results", tenant, operation_id, action, digest)
    }

    fn load_attendance_operation<T: DeserializeOwned>(&self, tenant: TenantId, operation_id: OperationId, action: &str, digest: &str) -> Result<Option<T>, StoreError> {
        Self::load_scoped_operation(&self.conn, "attendance_operation_results", tenant, operation_id, action, digest)
    }

    fn load_scoped_operation<T: DeserializeOwned>(conn: &Connection, table: &str, tenant: TenantId, operation_id: OperationId, action: &str, digest: &str) -> Result<Option<T>, StoreError> {
        let sql = format!("SELECT action,request_sha256,result_json FROM {table} WHERE tenant_id=?1 AND operation_id=?2");
        let stored: Option<(String,String,String)> = conn.query_row(&sql, params![tenant.to_string(), operation_id.to_string()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        match stored {
            None => Ok(None),
            Some((stored_action, stored_digest, json)) if stored_action==action && stored_digest==digest => Ok(Some(serde_json::from_str(&json)?)),
            Some(_) => Err(StoreError::Conflict("operation ID reused with different action or payload".into())),
        }
    }

    fn record_scoped_operation<T: Serialize>(tx: &Transaction<'_>, table: &str, tenant: TenantId, operation_id: OperationId, action: &str, digest: &str, result: &T, now: DateTime<Utc>) -> Result<(), StoreError> {
        let sql = format!("INSERT INTO {table}(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)");
        tx.execute(&sql, params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()])?;
        Ok(())
    }
}
