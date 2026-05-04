use crate::protocol::ServerMessage;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type Tx = mpsc::UnboundedSender<ServerMessage>;
pub type ConnectionId = u64;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryMessage {
    pub side: String,
    pub phase: String,
    pub body: String,
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn allocate_nameplate(&self, appid: &str, side: &str) -> Result<String>;
    async fn claim_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> Result<Option<String>>;
    async fn release_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> Result<()>;
    async fn list_nameplates(&self, appid: &str) -> Result<Vec<String>>;
    async fn open_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
        tx: Tx,
    ) -> Result<Vec<HistoryMessage>>;
    async fn add_message(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        phase: &str,
        body: &str,
    ) -> Result<Vec<Tx>>;
    async fn close_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
    ) -> Result<()>;
    async fn drop_connection(
        &self,
        appid: &str,
        side: &str,
        claimed_nameplates: &[String],
        open_mailboxes: &[String],
        conn_id: ConnectionId,
    ) -> Result<()>;
}

pub type DynStore = Arc<dyn Store>;

// ---------- in-memory implementation ----------

#[derive(Default)]
struct AppState {
    nameplates: HashMap<String, NameplateEntry>,
    mailboxes: HashMap<String, MailboxEntry>,
}

struct NameplateEntry {
    mailbox_id: String,
    sides: HashSet<String>,
}

struct MailboxEntry {
    messages: Vec<HistoryMessage>,
    claimed_sides: HashSet<String>,
    open_listeners: HashMap<ConnectionId, Listener>,
}

struct Listener {
    #[allow(dead_code)]
    side: String,
    tx: Tx,
}

#[derive(Default)]
pub struct InMemoryStore {
    apps: Mutex<HashMap<String, AppState>>,
}

impl InMemoryStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn allocate_nameplate(&self, appid: &str, side: &str) -> Result<String> {
        let mut apps = self.apps.lock().unwrap();
        let app = apps.entry(appid.to_string()).or_default();
        let mailbox_id = new_mailbox_id();
        loop {
            let candidate = random_nameplate();
            if !app.nameplates.contains_key(&candidate) {
                app.nameplates.insert(
                    candidate.clone(),
                    NameplateEntry {
                        mailbox_id: mailbox_id.clone(),
                        sides: HashSet::from([side.to_string()]),
                    },
                );
                app.mailboxes.insert(
                    mailbox_id.clone(),
                    MailboxEntry {
                        messages: Vec::new(),
                        claimed_sides: HashSet::from([side.to_string()]),
                        open_listeners: HashMap::new(),
                    },
                );
                return Ok(candidate);
            }
        }
    }

    async fn claim_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> Result<Option<String>> {
        let mut apps = self.apps.lock().unwrap();
        let app = apps.entry(appid.to_string()).or_default();
        let Some(entry) = app.nameplates.get_mut(nameplate) else { return Ok(None) };
        entry.sides.insert(side.to_string());
        let mailbox_id = entry.mailbox_id.clone();
        let mb = app.mailboxes.entry(mailbox_id.clone()).or_insert(MailboxEntry {
            messages: Vec::new(),
            claimed_sides: HashSet::new(),
            open_listeners: HashMap::new(),
        });
        mb.claimed_sides.insert(side.to_string());
        Ok(Some(mailbox_id))
    }

    async fn release_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> Result<()> {
        let mut apps = self.apps.lock().unwrap();
        if let Some(app) = apps.get_mut(appid) {
            if let Some(entry) = app.nameplates.get_mut(nameplate) {
                entry.sides.remove(side);
                if entry.sides.is_empty() {
                    app.nameplates.remove(nameplate);
                }
            }
        }
        Ok(())
    }

    async fn list_nameplates(&self, appid: &str) -> Result<Vec<String>> {
        let apps = self.apps.lock().unwrap();
        Ok(apps
            .get(appid)
            .map(|a| a.nameplates.keys().cloned().collect())
            .unwrap_or_default())
    }

    async fn open_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
        tx: Tx,
    ) -> Result<Vec<HistoryMessage>> {
        let mut apps = self.apps.lock().unwrap();
        let app = apps.entry(appid.to_string()).or_default();
        let mb = app.mailboxes.entry(mailbox_id.to_string()).or_insert(MailboxEntry {
            messages: Vec::new(),
            claimed_sides: HashSet::new(),
            open_listeners: HashMap::new(),
        });
        mb.claimed_sides.insert(side.to_string());
        mb.open_listeners.insert(conn_id, Listener { side: side.to_string(), tx });
        Ok(mb.messages.clone())
    }

    async fn add_message(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        phase: &str,
        body: &str,
    ) -> Result<Vec<Tx>> {
        let mut apps = self.apps.lock().unwrap();
        let Some(app) = apps.get_mut(appid) else { return Ok(Vec::new()) };
        let Some(mb) = app.mailboxes.get_mut(mailbox_id) else { return Ok(Vec::new()) };
        mb.messages.push(HistoryMessage {
            side: side.to_string(),
            phase: phase.to_string(),
            body: body.to_string(),
        });
        Ok(mb.open_listeners.values().map(|l| l.tx.clone()).collect())
    }

    async fn close_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
    ) -> Result<()> {
        let mut apps = self.apps.lock().unwrap();
        let Some(app) = apps.get_mut(appid) else { return Ok(()) };
        let Some(mb) = app.mailboxes.get_mut(mailbox_id) else { return Ok(()) };
        mb.open_listeners.remove(&conn_id);
        mb.claimed_sides.remove(side);
        if mb.claimed_sides.is_empty() {
            app.mailboxes.remove(mailbox_id);
        }
        Ok(())
    }

    async fn drop_connection(
        &self,
        appid: &str,
        side: &str,
        claimed_nameplates: &[String],
        open_mailboxes: &[String],
        conn_id: ConnectionId,
    ) -> Result<()> {
        for n in claimed_nameplates {
            self.release_nameplate(appid, n, side).await?;
        }
        for m in open_mailboxes {
            self.close_mailbox(appid, m, side, conn_id).await?;
        }
        Ok(())
    }
}

pub(crate) fn new_mailbox_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(crate) fn random_nameplate() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen_range(1..1000);
    n.to_string()
}

#[cfg(test)]
mod test {
    use super::*;

    fn channel() -> (Tx, mpsc::UnboundedReceiver<ServerMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (tx, rx)
    }

    #[tokio::test]
    async fn allocate_then_claim_yields_same_mailbox() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        let mb = s.claim_nameplate("app", &np, "bob").await.unwrap().expect("claim");
        let (tx_a, _ra) = channel();
        let (tx_b, _rb) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a).await.unwrap();
        s.open_mailbox("app", &mb, "bob", 2, tx_b).await.unwrap();
        let recipients = s.add_message("app", &mb, "alice", "pake", "ff").await.unwrap();
        assert_eq!(recipients.len(), 2, "both sides should be listening");
    }

    #[tokio::test]
    async fn open_replays_history() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        let mb = s.claim_nameplate("app", &np, "bob").await.unwrap().unwrap();
        let (tx_a, _ra) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a).await.unwrap();
        s.add_message("app", &mb, "alice", "pake", "aa").await.unwrap();
        let (tx_b, _rb) = channel();
        let history = s.open_mailbox("app", &mb, "bob", 2, tx_b).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0],
            HistoryMessage { side: "alice".into(), phase: "pake".into(), body: "aa".into() }
        );
    }

    #[tokio::test]
    async fn release_removes_nameplate_when_last_side_leaves() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        s.claim_nameplate("app", &np, "bob").await.unwrap().unwrap();
        s.release_nameplate("app", &np, "alice").await.unwrap();
        s.release_nameplate("app", &np, "bob").await.unwrap();
        assert!(s.list_nameplates("app").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn close_removes_mailbox_when_last_side_leaves() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        let mb = s.claim_nameplate("app", &np, "bob").await.unwrap().unwrap();
        let (tx_a, _ra) = channel();
        let (tx_b, _rb) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a).await.unwrap();
        s.open_mailbox("app", &mb, "bob", 2, tx_b).await.unwrap();
        s.close_mailbox("app", &mb, "alice", 1).await.unwrap();
        s.close_mailbox("app", &mb, "bob", 2).await.unwrap();
        let recipients = s.add_message("app", &mb, "alice", "pake", "ff").await.unwrap();
        assert!(recipients.is_empty());
    }

    #[tokio::test]
    async fn claim_unknown_nameplate_returns_none() {
        let s = InMemoryStore::new();
        let result = s.claim_nameplate("app", "9999", "alice").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn drop_connection_releases_nameplate_and_closes_mailbox() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        let mb = s.claim_nameplate("app", &np, "alice").await.unwrap().unwrap();
        let (tx_a, _ra) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a).await.unwrap();

        s.drop_connection("app", "alice", &[np.clone()], &[mb.clone()], 1).await.unwrap();

        // Nameplate gone (alice was the only side).
        assert!(s.list_nameplates("app").await.unwrap().is_empty());
        // Mailbox gone too — adding now is a no-op.
        let recipients = s.add_message("app", &mb, "alice", "pake", "00").await.unwrap();
        assert!(recipients.is_empty());
    }

    #[tokio::test]
    async fn list_nameplates_is_isolated_per_app() {
        let s = InMemoryStore::new();
        s.allocate_nameplate("app1", "alice").await.unwrap();
        s.allocate_nameplate("app2", "alice").await.unwrap();
        s.allocate_nameplate("app2", "bob").await.unwrap();
        assert_eq!(s.list_nameplates("app1").await.unwrap().len(), 1);
        assert_eq!(s.list_nameplates("app2").await.unwrap().len(), 2);
        assert!(s.list_nameplates("app3").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_message_to_unknown_mailbox_is_noop() {
        let s = InMemoryStore::new();
        let recipients = s.add_message("app", "missing-mailbox-id", "alice", "pake", "ff")
            .await
            .unwrap();
        assert!(recipients.is_empty());
    }

    #[tokio::test]
    async fn second_close_for_same_side_is_idempotent() {
        let s = InMemoryStore::new();
        let np = s.allocate_nameplate("app", "alice").await.unwrap();
        let mb = s.claim_nameplate("app", &np, "alice").await.unwrap().unwrap();
        let (tx, _rx) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx).await.unwrap();
        s.close_mailbox("app", &mb, "alice", 1).await.unwrap();
        s.close_mailbox("app", &mb, "alice", 1).await.unwrap(); // no panic
    }
}
