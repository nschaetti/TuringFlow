use rusqlite::params;
use serde_json::Value;

use crate::kernel::context::ExecutionContext;
use crate::kernel::errors::KernelError;
use crate::tfpv1::storage::sqlite::open_connection;

#[derive(Debug, Clone)]
pub struct UserIngestReq {
    pub channel: String,
    pub thread_id: Option<String>,
    pub body: String,
    pub external_message_id: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct UserIngestResp {
    pub message_id: String,
    pub received_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UserRecvReq {
    pub limit: usize,
    pub consume: bool,
}

#[derive(Debug, Clone)]
pub struct UserInboundMessage {
    pub message_id: String,
    pub channel: String,
    pub thread_id: Option<String>,
    pub body: String,
    pub metadata: Option<Value>,
    pub received_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UserRecvResp {
    pub messages: Vec<UserInboundMessage>,
}

#[derive(Debug, Clone)]
pub struct UserSendReq {
    pub channel: Option<String>,
    pub thread_id: Option<String>,
    pub body: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct UserSendResp {
    pub message_id: String,
    pub channel: String,
    pub queued_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UserInboxReq {
    pub limit: usize,
    pub include_delivered: bool,
}

#[derive(Debug, Clone)]
pub struct UserOutboundMessage {
    pub message_id: String,
    pub channel: String,
    pub thread_id: Option<String>,
    pub body: String,
    pub metadata: Option<Value>,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UserInboxResp {
    pub messages: Vec<UserOutboundMessage>,
}

#[derive(Debug, Clone)]
pub struct UserRouteResolveReq {
    pub thread_id: Option<String>,
    pub preferred_channel: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserRouteResolveResp {
    pub channel: String,
}

pub trait UserCommsProvider: Send + Sync {
    fn ingest(
        &self,
        ctx: &ExecutionContext,
        req: UserIngestReq,
    ) -> Result<UserIngestResp, KernelError>;
    fn recv(&self, ctx: &ExecutionContext, req: UserRecvReq) -> Result<UserRecvResp, KernelError>;
    fn send(&self, ctx: &ExecutionContext, req: UserSendReq) -> Result<UserSendResp, KernelError>;
    fn inbox(
        &self,
        ctx: &ExecutionContext,
        req: UserInboxReq,
    ) -> Result<UserInboxResp, KernelError>;
    fn route_resolve(
        &self,
        ctx: &ExecutionContext,
        req: UserRouteResolveReq,
    ) -> Result<UserRouteResolveResp, KernelError>;
}

#[derive(Debug, Clone)]
pub struct SqliteUserCommsProvider {
    db_path: String,
}

impl SqliteUserCommsProvider {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }
}

impl UserCommsProvider for SqliteUserCommsProvider {
    fn ingest(
        &self,
        _ctx: &ExecutionContext,
        req: UserIngestReq,
    ) -> Result<UserIngestResp, KernelError> {
        let channel = normalize_channel(&req.channel)?;
        let body = normalize_body(&req.body)?;
        let now = now_epoch_ms();
        let message_id = req
            .external_message_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| unique_message_id("usr_in"));
        let metadata_json = req.metadata.as_ref().map(value_to_text);

        let conn = open_connection(&self.db_path)
            .map_err(|error| KernelError::internal(error.to_string()))?;

        conn.execute(
            "
            INSERT INTO user_inbound (
                message_id,
                channel,
                thread_id,
                body,
                metadata_json,
                external_message_id,
                received_at_ms,
                acknowledged,
                acknowledged_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL)
            ON CONFLICT(message_id) DO NOTHING
            ",
            params![
                message_id,
                channel,
                req.thread_id,
                body,
                metadata_json,
                req.external_message_id,
                now
            ],
        )
        .map_err(|error| KernelError::internal(error.to_string()))?;

        Ok(UserIngestResp {
            message_id,
            received_at_ms: now,
        })
    }

    fn recv(&self, _ctx: &ExecutionContext, req: UserRecvReq) -> Result<UserRecvResp, KernelError> {
        let limit = normalize_limit(req.limit);
        let mut conn = open_connection(&self.db_path)
            .map_err(|error| KernelError::internal(error.to_string()))?;

        let tx = conn
            .transaction()
            .map_err(|error| KernelError::internal(error.to_string()))?;

        let messages = {
            let mut stmt = tx
                .prepare(
                    "
                    SELECT message_id, channel, thread_id, body, metadata_json, received_at_ms
                    FROM user_inbound
                    WHERE acknowledged = 0
                    ORDER BY received_at_ms ASC
                    LIMIT ?1
                    ",
                )
                .map_err(|error| KernelError::internal(error.to_string()))?;

            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let metadata_raw: Option<String> = row.get(4)?;
                    Ok(UserInboundMessage {
                        message_id: row.get(0)?,
                        channel: row.get(1)?,
                        thread_id: row.get(2)?,
                        body: row.get(3)?,
                        metadata: metadata_raw
                            .as_deref()
                            .and_then(|raw| serde_json::from_str::<Value>(raw).ok()),
                        received_at_ms: row.get(5)?,
                    })
                })
                .map_err(|error| KernelError::internal(error.to_string()))?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(|error| KernelError::internal(error.to_string()))?);
            }
            out
        };

        if req.consume && !messages.is_empty() {
            let now = now_epoch_ms();
            for message in &messages {
                tx.execute(
                    "
                    UPDATE user_inbound
                    SET acknowledged = 1,
                        acknowledged_at_ms = ?1
                    WHERE message_id = ?2
                    ",
                    params![now, message.message_id],
                )
                .map_err(|error| KernelError::internal(error.to_string()))?;
            }
        }

        tx.commit()
            .map_err(|error| KernelError::internal(error.to_string()))?;

        Ok(UserRecvResp { messages })
    }

    fn send(&self, ctx: &ExecutionContext, req: UserSendReq) -> Result<UserSendResp, KernelError> {
        let body = normalize_body(&req.body)?;
        let route = self.route_resolve(
            ctx,
            UserRouteResolveReq {
                thread_id: req.thread_id.clone(),
                preferred_channel: req.channel.clone(),
            },
        )?;

        let now = now_epoch_ms();
        let message_id = unique_message_id("usr_out");
        let metadata_json = req.metadata.as_ref().map(value_to_text);
        let conn = open_connection(&self.db_path)
            .map_err(|error| KernelError::internal(error.to_string()))?;

        conn.execute(
            "
            INSERT INTO user_outbound (
                message_id,
                channel,
                thread_id,
                body,
                metadata_json,
                status,
                created_at_ms,
                updated_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7)
            ",
            params![
                message_id,
                route.channel,
                req.thread_id,
                body,
                metadata_json,
                now,
                now
            ],
        )
        .map_err(|error| KernelError::internal(error.to_string()))?;

        Ok(UserSendResp {
            message_id,
            channel: route.channel,
            queued_at_ms: now,
        })
    }

    fn inbox(
        &self,
        _ctx: &ExecutionContext,
        req: UserInboxReq,
    ) -> Result<UserInboxResp, KernelError> {
        let conn = open_connection(&self.db_path)
            .map_err(|error| KernelError::internal(error.to_string()))?;
        let limit = normalize_limit(req.limit);

        let sql = if req.include_delivered {
            "
            SELECT message_id, channel, thread_id, body, metadata_json, status, created_at_ms, updated_at_ms
            FROM user_outbound
            ORDER BY created_at_ms DESC
            LIMIT ?1
            "
        } else {
            "
            SELECT message_id, channel, thread_id, body, metadata_json, status, created_at_ms, updated_at_ms
            FROM user_outbound
            WHERE status != 'delivered'
            ORDER BY created_at_ms DESC
            LIMIT ?1
            "
        };

        let mut stmt = conn
            .prepare(sql)
            .map_err(|error| KernelError::internal(error.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let metadata_raw: Option<String> = row.get(4)?;
                Ok(UserOutboundMessage {
                    message_id: row.get(0)?,
                    channel: row.get(1)?,
                    thread_id: row.get(2)?,
                    body: row.get(3)?,
                    metadata: metadata_raw
                        .as_deref()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok()),
                    status: row.get(5)?,
                    created_at_ms: row.get(6)?,
                    updated_at_ms: row.get(7)?,
                })
            })
            .map_err(|error| KernelError::internal(error.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(|error| KernelError::internal(error.to_string()))?);
        }

        Ok(UserInboxResp { messages })
    }

    fn route_resolve(
        &self,
        _ctx: &ExecutionContext,
        req: UserRouteResolveReq,
    ) -> Result<UserRouteResolveResp, KernelError> {
        if let Some(channel) = req.preferred_channel {
            return Ok(UserRouteResolveResp {
                channel: normalize_channel(&channel)?,
            });
        }

        if let Some(thread_id) = req.thread_id {
            let conn = open_connection(&self.db_path)
                .map_err(|error| KernelError::internal(error.to_string()))?;
            let channel: Option<String> = conn
                .query_row(
                    "
                    SELECT channel
                    FROM user_inbound
                    WHERE thread_id = ?1
                    ORDER BY received_at_ms DESC
                    LIMIT 1
                    ",
                    params![thread_id],
                    |row| row.get(0),
                )
                .ok();

            if let Some(channel) = channel {
                return Ok(UserRouteResolveResp { channel });
            }
        }

        Ok(UserRouteResolveResp {
            channel: "matrix".to_string(),
        })
    }
}

fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, 200)
}

fn normalize_body(body: &str) -> Result<String, KernelError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(KernelError::invalid("message body must not be empty"));
    }
    Ok(body.to_string())
}

fn normalize_channel(channel: &str) -> Result<String, KernelError> {
    let normalized = channel.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(KernelError::invalid("channel must not be empty"));
    }
    Ok(normalized)
}

fn value_to_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn now_epoch_ms() -> i64 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
}

fn unique_message_id(prefix: &str) -> String {
    format!(
        "{}_{}_{}_{}",
        prefix,
        std::process::id(),
        now_epoch_ms(),
        now_epoch_nanos_for_suffix()
    )
}

fn now_epoch_nanos_for_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}
