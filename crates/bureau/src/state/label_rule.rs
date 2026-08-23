//! Durable audit records and hourly bounds for label-rule updates.

use rusqlite::{Connection, Row};

use super::{Error, HOUR_MS, Store, now_millis, sql};

/// One label-rule update's immutable audit context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelRuleAudit {
    /// Unique identity shared by this attempt's events.
    pub attempt_id: String,
    /// Configured rule name.
    pub rule: String,
    /// Repository identity active when the attempt began.
    pub source: String,
    /// Forge work-item identity.
    pub item: String,
    /// Labels requested for addition.
    pub add_labels: Vec<String>,
    /// Labels requested for removal.
    pub remove_labels: Vec<String>,
    /// Blocking dependencies observed.
    pub dependency_count: u32,
    /// Closed blocking dependencies observed.
    pub closed_dependency_count: u32,
}

/// One durable label-rule event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelRuleEventKind {
    /// The bounded forge mutation is about to start.
    UpdateStarted,
    /// The forge accepted the mutation.
    UpdateApplied,
    /// The forge rejected or timed out the mutation.
    UpdateFailed,
    /// Recovery proved the work item no longer exists.
    UpdateAbandoned,
}

impl LabelRuleEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::UpdateStarted => "update_started",
            Self::UpdateApplied => "update_applied",
            Self::UpdateFailed => "update_failed",
            Self::UpdateAbandoned => "update_abandoned",
        }
    }

    fn from_str(value: &str) -> Result<Self, Error> {
        match value {
            "update_started" => Ok(Self::UpdateStarted),
            "update_applied" => Ok(Self::UpdateApplied),
            "update_failed" => Ok(Self::UpdateFailed),
            "update_abandoned" => Ok(Self::UpdateAbandoned),
            other => Err(Error::UnknownLabelRuleEvent(other.to_owned())),
        }
    }
}

/// A persisted label-rule audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelRuleEvent {
    /// Unique identity shared by this attempt's events.
    pub attempt_id: String,
    /// Configured rule name.
    pub rule: String,
    /// Repository identity active when the attempt began.
    pub source: String,
    /// Forge work-item identity.
    pub item: String,
    /// Event phase.
    pub kind: LabelRuleEventKind,
    /// Human-readable audit message.
    pub message: String,
    /// Labels requested for addition.
    pub add_labels: Vec<String>,
    /// Labels requested for removal.
    pub remove_labels: Vec<String>,
    /// Blocking dependencies observed.
    pub dependency_count: u32,
    /// Closed blocking dependencies observed.
    pub closed_dependency_count: u32,
    /// Event time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

fn labels(value: &str) -> Result<Vec<String>, Error> {
    Ok(serde_json::from_str(value)?)
}

fn event_from_row(row: &Row<'_>) -> Result<LabelRuleEvent, Error> {
    let kind = LabelRuleEventKind::from_str(&row.get::<_, String>(4)?)?;
    let occurred: i64 = row.get(10)?;
    Ok(LabelRuleEvent {
        attempt_id: row.get(0)?,
        rule: row.get(1)?,
        source: row.get(2)?,
        item: row.get(3)?,
        kind,
        message: row.get(5)?,
        add_labels: labels(&row.get::<_, String>(6)?)?,
        remove_labels: labels(&row.get::<_, String>(7)?)?,
        dependency_count: u32::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
        closed_dependency_count: u32::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
        occurred_at_ms: u64::try_from(occurred).unwrap_or(0),
    })
}

fn audit_from_row(row: &Row<'_>) -> Result<LabelRuleAudit, Error> {
    Ok(LabelRuleAudit {
        attempt_id: row.get(0)?,
        rule: row.get(1)?,
        source: row.get(2)?,
        item: row.get(3)?,
        add_labels: labels(&row.get::<_, String>(4)?)?,
        remove_labels: labels(&row.get::<_, String>(5)?)?,
        dependency_count: u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        closed_dependency_count: u32::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
    })
}

fn insert(
    conn: &Connection,
    audit: &LabelRuleAudit,
    kind: LabelRuleEventKind,
    message: &str,
    now: i64,
) -> Result<(), Error> {
    let add = serde_json::to_string(&audit.add_labels)?;
    let remove = serde_json::to_string(&audit.remove_labels)?;
    let params = (
        &audit.attempt_id,
        &audit.rule,
        &audit.source,
        &audit.item,
        kind.as_str(),
        message,
        add,
        remove,
        audit.dependency_count,
        audit.closed_dependency_count,
        now,
    );
    conn.execute(sql::INSERT_LABEL_RULE_EVENT, params)?;
    Ok(())
}

fn begin(
    conn: &mut Connection,
    audit: &LabelRuleAudit,
    max: u32,
    message: &str,
) -> Result<bool, Error> {
    let now = now_millis();
    let tx = conn.transaction()?;
    let used: u32 = tx.query_row(
        sql::LABEL_RULE_UPDATES_SINCE,
        (&audit.rule, now - HOUR_MS),
        |row| row.get(0),
    )?;
    if used >= max {
        return Ok(false);
    }
    insert(&tx, audit, LabelRuleEventKind::UpdateStarted, message, now)?;
    tx.commit()?;
    Ok(true)
}

fn supersede(
    conn: &mut Connection,
    old: &LabelRuleAudit,
    new: &LabelRuleAudit,
    max: u32,
    messages: (&str, &str),
) -> Result<bool, Error> {
    let now = now_millis();
    let tx = conn.transaction()?;
    let used: u32 = tx.query_row(
        sql::LABEL_RULE_UPDATES_SINCE,
        (&new.rule, now - HOUR_MS),
        |row| row.get(0),
    )?;
    if used >= max {
        return Ok(false);
    }
    insert(
        &tx,
        old,
        LabelRuleEventKind::UpdateAbandoned,
        messages.0,
        now,
    )?;
    insert(&tx, new, LabelRuleEventKind::UpdateStarted, messages.1, now)?;
    tx.commit()?;
    Ok(true)
}

fn events(conn: &Connection, rule: &str) -> Result<Vec<LabelRuleEvent>, Error> {
    let mut statement = conn.prepare(sql::LABEL_RULE_EVENTS)?;
    let rows = statement.query_map((rule,), |row| {
        event_from_row(row).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn pending(conn: &Connection, rule: &str) -> Result<Vec<LabelRuleAudit>, Error> {
    let mut statement = conn.prepare(sql::PENDING_LABEL_RULE_UPDATES)?;
    let rows = statement.query_map((rule,), |row| {
        audit_from_row(row).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

impl Store {
    /// Atomically reserves hourly update headroom and records `update_started`.
    ///
    /// # Errors
    /// Propagates serialization and `SQLite` failures.
    pub fn begin_label_rule_update(
        &self,
        audit: &LabelRuleAudit,
        max: u32,
        message: &str,
    ) -> Result<bool, Error> {
        begin(&mut self.lock(), audit, max, message)
    }

    /// Atomically supersedes an old attempt while reserving a bounded retry.
    ///
    /// # Errors
    /// Propagates serialization and `SQLite` failures.
    pub fn supersede_label_rule_update(
        &self,
        old: &LabelRuleAudit,
        new: &LabelRuleAudit,
        max: u32,
        messages: (&str, &str),
    ) -> Result<bool, Error> {
        supersede(&mut self.lock(), old, new, max, messages)
    }

    /// Appends a terminal audit event for one attempted label update.
    ///
    /// # Errors
    /// Propagates serialization and `SQLite` failures.
    pub fn record_label_rule_event(
        &self,
        audit: &LabelRuleAudit,
        kind: LabelRuleEventKind,
        message: &str,
    ) -> Result<(), Error> {
        insert(&self.lock(), audit, kind, message, now_millis())
    }

    /// Reads one rule's durable audit history in occurrence order.
    ///
    /// # Errors
    /// Propagates decoding and `SQLite` failures.
    pub fn label_rule_events(&self, rule: &str) -> Result<Vec<LabelRuleEvent>, Error> {
        events(&self.lock(), rule)
    }

    /// Reads each item whose latest attempted mutation is not applied or abandoned.
    ///
    /// # Errors
    /// Propagates decoding and `SQLite` failures.
    pub fn pending_label_rule_updates(&self, rule: &str) -> Result<Vec<LabelRuleAudit>, Error> {
        pending(&self.lock(), rule)
    }

    /// Whether one attempt is still the latest unresolved update for its item.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn label_rule_update_pending(&self, attempt_id: &str) -> Result<bool, Error> {
        Ok(self
            .lock()
            .query_row(sql::LABEL_RULE_UPDATE_PENDING, (attempt_id,), |row| {
                row.get(0)
            })?)
    }
}
