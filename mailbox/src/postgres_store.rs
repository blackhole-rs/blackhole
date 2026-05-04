use crate::state::{
    ConnectionId, DynStore, HistoryMessage, Result as StoreResult, Store, StoreError, Tx,
    new_mailbox_id, random_nameplate,
};
use async_trait::async_trait;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type ListenerKey = (String, String); // (appid, mailbox_id)

pub struct PostgresStore {
    pool: PgPool,
    listeners: Mutex<HashMap<ListenerKey, HashMap<ConnectionId, Listener>>>,
}

struct Listener {
    #[allow(dead_code)]
    side: String,
    tx: Tx,
}

impl PostgresStore {
    pub async fn connect(url: &str) -> anyhow::Result<DynStore> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Arc::new(Self {
            pool,
            listeners: Mutex::new(HashMap::new()),
        }))
    }

    fn add_listener(&self, key: ListenerKey, conn_id: ConnectionId, side: String, tx: Tx) {
        self.listeners
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .insert(conn_id, Listener { side, tx });
    }

    fn remove_listener(&self, key: &ListenerKey, conn_id: ConnectionId) {
        let mut all = self.listeners.lock().unwrap();
        if let Some(m) = all.get_mut(key) {
            m.remove(&conn_id);
            if m.is_empty() {
                all.remove(key);
            }
        }
    }

    fn listeners_for(&self, key: &ListenerKey) -> Vec<Tx> {
        self.listeners
            .lock()
            .unwrap()
            .get(key)
            .map(|m| m.values().map(|l| l.tx.clone()).collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Store for PostgresStore {
    async fn allocate_nameplate(&self, appid: &str, side: &str) -> StoreResult<String> {
        // Retry on collision; in practice nameplate space (1..1000) is large vs concurrent allocations.
        for _ in 0..32 {
            let candidate = random_nameplate();
            let mailbox_id = new_mailbox_id();
            let mut tx = self.pool.begin().await?;
            let inserted: Option<(String,)> = sqlx::query_as(
                "INSERT INTO nameplates (appid, nameplate, mailbox_id, sides) \
                 VALUES ($1, $2, $3, ARRAY[$4]::text[]) \
                 ON CONFLICT DO NOTHING \
                 RETURNING nameplate",
            )
            .bind(appid)
            .bind(&candidate)
            .bind(&mailbox_id)
            .bind(side)
            .fetch_optional(&mut *tx)
            .await?;

            if inserted.is_some() {
                sqlx::query(
                    "INSERT INTO mailboxes (appid, mailbox_id, claimed_sides) \
                     VALUES ($1, $2, ARRAY[$3]::text[]) \
                     ON CONFLICT DO NOTHING",
                )
                .bind(appid)
                .bind(&mailbox_id)
                .bind(side)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                return Ok(candidate);
            }
            tx.rollback().await?;
        }
        Err(StoreError::Db(sqlx::Error::Configuration(
            "could not allocate nameplate after retries".into(),
        )))
    }

    async fn claim_nameplate(
        &self,
        appid: &str,
        nameplate: &str,
        side: &str,
    ) -> StoreResult<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(String,)> = sqlx::query_as(
            "UPDATE nameplates SET sides = \
             CASE WHEN $3 = ANY(sides) THEN sides ELSE array_append(sides, $3) END \
             WHERE appid = $1 AND nameplate = $2 \
             RETURNING mailbox_id",
        )
        .bind(appid)
        .bind(nameplate)
        .bind(side)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((mailbox_id,)) = row else {
            tx.rollback().await?;
            return Ok(None);
        };

        sqlx::query(
            "INSERT INTO mailboxes (appid, mailbox_id, claimed_sides) \
             VALUES ($1, $2, ARRAY[$3]::text[]) \
             ON CONFLICT (appid, mailbox_id) DO UPDATE SET claimed_sides = \
             CASE WHEN $3 = ANY(mailboxes.claimed_sides) THEN mailboxes.claimed_sides \
             ELSE array_append(mailboxes.claimed_sides, $3) END",
        )
        .bind(appid)
        .bind(&mailbox_id)
        .bind(side)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(mailbox_id))
    }

    async fn release_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE nameplates SET sides = array_remove(sides, $3) \
             WHERE appid = $1 AND nameplate = $2",
        )
        .bind(appid)
        .bind(nameplate)
        .bind(side)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM nameplates \
             WHERE appid = $1 AND nameplate = $2 AND cardinality(sides) = 0",
        )
        .bind(appid)
        .bind(nameplate)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_nameplates(&self, appid: &str) -> StoreResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT nameplate FROM nameplates WHERE appid = $1 ORDER BY nameplate",
        )
        .bind(appid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    async fn open_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
        tx: Tx,
    ) -> StoreResult<Vec<HistoryMessage>> {
        sqlx::query(
            "INSERT INTO mailboxes (appid, mailbox_id, claimed_sides) \
             VALUES ($1, $2, ARRAY[$3]::text[]) \
             ON CONFLICT (appid, mailbox_id) DO UPDATE SET claimed_sides = \
             CASE WHEN $3 = ANY(mailboxes.claimed_sides) THEN mailboxes.claimed_sides \
             ELSE array_append(mailboxes.claimed_sides, $3) END",
        )
        .bind(appid)
        .bind(mailbox_id)
        .bind(side)
        .execute(&self.pool)
        .await?;

        self.add_listener(
            (appid.to_string(), mailbox_id.to_string()),
            conn_id,
            side.to_string(),
            tx,
        );

        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT side, phase, body FROM mailbox_messages \
             WHERE appid = $1 AND mailbox_id = $2 ORDER BY id",
        )
        .bind(appid)
        .bind(mailbox_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(side, phase, body)| HistoryMessage { side, phase, body })
            .collect())
    }

    async fn add_message(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        phase: &str,
        body: &str,
    ) -> StoreResult<Vec<Tx>> {
        // Only persist if the mailbox still exists. Otherwise drop silently like the in-memory impl.
        let exists: Option<(i64,)> = sqlx::query_as(
            "SELECT 1::bigint FROM mailboxes WHERE appid = $1 AND mailbox_id = $2",
        )
        .bind(appid)
        .bind(mailbox_id)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            return Ok(Vec::new());
        }
        sqlx::query(
            "INSERT INTO mailbox_messages (appid, mailbox_id, side, phase, body) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(appid)
        .bind(mailbox_id)
        .bind(side)
        .bind(phase)
        .bind(body)
        .execute(&self.pool)
        .await?;
        Ok(self.listeners_for(&(appid.to_string(), mailbox_id.to_string())))
    }

    async fn close_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
    ) -> StoreResult<()> {
        self.remove_listener(&(appid.to_string(), mailbox_id.to_string()), conn_id);

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE mailboxes SET claimed_sides = array_remove(claimed_sides, $3) \
             WHERE appid = $1 AND mailbox_id = $2",
        )
        .bind(appid)
        .bind(mailbox_id)
        .bind(side)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM mailbox_messages WHERE appid = $1 AND mailbox_id = $2 \
             AND NOT EXISTS (SELECT 1 FROM mailboxes m \
                WHERE m.appid = $1 AND m.mailbox_id = $2 AND cardinality(m.claimed_sides) > 0)",
        )
        .bind(appid)
        .bind(mailbox_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM mailboxes \
             WHERE appid = $1 AND mailbox_id = $2 AND cardinality(claimed_sides) = 0",
        )
        .bind(appid)
        .bind(mailbox_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn drop_connection(
        &self,
        appid: &str,
        side: &str,
        claimed_nameplates: &[String],
        open_mailboxes: &[String],
        conn_id: ConnectionId,
    ) -> StoreResult<()> {
        for n in claimed_nameplates {
            self.release_nameplate(appid, n, side).await?;
        }
        for m in open_mailboxes {
            self.close_mailbox(appid, m, side, conn_id).await?;
        }
        Ok(())
    }
}
