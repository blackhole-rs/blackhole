use crate::protocol::ServerMessage;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type Tx = mpsc::UnboundedSender<ServerMessage>;
pub type ConnectionId = u64;

#[derive(Default)]
pub struct GlobalState {
    apps: HashMap<String, AppState>,
}

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
    messages: Vec<StoredMessage>,
    claimed_sides: HashSet<String>,
    open_listeners: HashMap<ConnectionId, Listener>,
}

struct Listener {
    #[allow(dead_code)]
    side: String,
    tx: Tx,
}

#[derive(Clone)]
struct StoredMessage {
    side: String,
    phase: String,
    body: String,
}

pub struct Shared(Mutex<GlobalState>);

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(GlobalState::default())))
    }

    pub fn allocate_nameplate(&self, appid: &str, side: &str) -> String {
        let mut g = self.0.lock().unwrap();
        let app = g.apps.entry(appid.to_string()).or_default();
        let mailbox_id = new_id();
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
                return candidate;
            }
        }
    }

    /// Claim a nameplate. Returns the linked mailbox id, or `None` if the nameplate
    /// doesn't exist (caller should produce an error to the client).
    pub fn claim_nameplate(&self, appid: &str, nameplate: &str, side: &str) -> Option<String> {
        let mut g = self.0.lock().unwrap();
        let app = g.apps.entry(appid.to_string()).or_default();
        let entry = app.nameplates.get_mut(nameplate)?;
        entry.sides.insert(side.to_string());
        let mailbox_id = entry.mailbox_id.clone();
        let mb = app.mailboxes.entry(mailbox_id.clone()).or_insert(MailboxEntry {
            messages: Vec::new(),
            claimed_sides: HashSet::new(),
            open_listeners: HashMap::new(),
        });
        mb.claimed_sides.insert(side.to_string());
        Some(mailbox_id)
    }

    pub fn release_nameplate(&self, appid: &str, nameplate: &str, side: &str) {
        let mut g = self.0.lock().unwrap();
        if let Some(app) = g.apps.get_mut(appid) {
            if let Some(entry) = app.nameplates.get_mut(nameplate) {
                entry.sides.remove(side);
                if entry.sides.is_empty() {
                    app.nameplates.remove(nameplate);
                }
            }
        }
    }

    pub fn list_nameplates(&self, appid: &str) -> Vec<String> {
        let g = self.0.lock().unwrap();
        g.apps
            .get(appid)
            .map(|a| a.nameplates.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Open a mailbox for a connection. Returns the message log to replay.
    pub fn open_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
        tx: Tx,
    ) -> Vec<(String, String, String)> {
        let mut g = self.0.lock().unwrap();
        let app = g.apps.entry(appid.to_string()).or_default();
        let mb = app.mailboxes.entry(mailbox_id.to_string()).or_insert(MailboxEntry {
            messages: Vec::new(),
            claimed_sides: HashSet::new(),
            open_listeners: HashMap::new(),
        });
        mb.claimed_sides.insert(side.to_string());
        mb.open_listeners.insert(conn_id, Listener { side: side.to_string(), tx });
        mb.messages
            .iter()
            .map(|m| (m.side.clone(), m.phase.clone(), m.body.clone()))
            .collect()
    }

    /// Add a message to the mailbox; returns recipients (sides + their tx) including the sender.
    pub fn add_message(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        phase: &str,
        body: &str,
    ) -> Vec<Tx> {
        let mut g = self.0.lock().unwrap();
        let Some(app) = g.apps.get_mut(appid) else { return Vec::new() };
        let Some(mb) = app.mailboxes.get_mut(mailbox_id) else { return Vec::new() };
        mb.messages.push(StoredMessage {
            side: side.to_string(),
            phase: phase.to_string(),
            body: body.to_string(),
        });
        mb.open_listeners.values().map(|l| l.tx.clone()).collect()
    }

    /// Close a mailbox for a side. Removes their listener entry; if no claimed sides remain, drops the mailbox.
    pub fn close_mailbox(
        &self,
        appid: &str,
        mailbox_id: &str,
        side: &str,
        conn_id: ConnectionId,
    ) {
        let mut g = self.0.lock().unwrap();
        let Some(app) = g.apps.get_mut(appid) else { return };
        let Some(mb) = app.mailboxes.get_mut(mailbox_id) else { return };
        mb.open_listeners.remove(&conn_id);
        mb.claimed_sides.remove(side);
        if mb.claimed_sides.is_empty() {
            app.mailboxes.remove(mailbox_id);
        }
    }

    /// Cleanup when a connection drops without orderly release/close.
    pub fn drop_connection(
        &self,
        appid: &str,
        side: &str,
        claimed_nameplates: &[String],
        open_mailboxes: &[String],
        conn_id: ConnectionId,
    ) {
        for n in claimed_nameplates {
            self.release_nameplate(appid, n, side);
        }
        for m in open_mailboxes {
            self.close_mailbox(appid, m, side, conn_id);
        }
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn random_nameplate() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen_range(1..1000);
    n.to_string()
}

#[cfg(test)]
mod test {
    use super::*;

    fn make() -> Arc<Shared> {
        Shared::new()
    }

    fn channel() -> (Tx, mpsc::UnboundedReceiver<ServerMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (tx, rx)
    }

    #[test]
    fn allocate_then_claim_yields_same_mailbox() {
        let s = make();
        let np = s.allocate_nameplate("app", "alice");
        let mb = s.claim_nameplate("app", &np, "bob").expect("claim");
        // alice already had it claimed via allocate; bob just joined
        let (tx_a, _ra) = channel();
        let (tx_b, _rb) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a);
        s.open_mailbox("app", &mb, "bob", 2, tx_b);
        let recipients = s.add_message("app", &mb, "alice", "pake", "ff");
        assert_eq!(recipients.len(), 2, "both sides should be listening");
    }

    #[test]
    fn open_replays_history() {
        let s = make();
        let np = s.allocate_nameplate("app", "alice");
        let mb = s.claim_nameplate("app", &np, "bob").unwrap();
        let (tx_a, _ra) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a);
        s.add_message("app", &mb, "alice", "pake", "aa");
        // bob opens later; should get full replay
        let (tx_b, _rb) = channel();
        let history = s.open_mailbox("app", &mb, "bob", 2, tx_b);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], ("alice".to_string(), "pake".to_string(), "aa".to_string()));
    }

    #[test]
    fn release_removes_nameplate_when_last_side_leaves() {
        let s = make();
        let np = s.allocate_nameplate("app", "alice");
        s.claim_nameplate("app", &np, "bob").unwrap();
        s.release_nameplate("app", &np, "alice");
        s.release_nameplate("app", &np, "bob");
        assert!(s.list_nameplates("app").is_empty());
    }

    #[test]
    fn close_removes_mailbox_when_last_side_leaves() {
        let s = make();
        let np = s.allocate_nameplate("app", "alice");
        let mb = s.claim_nameplate("app", &np, "bob").unwrap();
        let (tx_a, _ra) = channel();
        let (tx_b, _rb) = channel();
        s.open_mailbox("app", &mb, "alice", 1, tx_a);
        s.open_mailbox("app", &mb, "bob", 2, tx_b);
        s.close_mailbox("app", &mb, "alice", 1);
        s.close_mailbox("app", &mb, "bob", 2);
        // adding now is a no-op (mailbox gone)
        let recipients = s.add_message("app", &mb, "alice", "pake", "ff");
        assert!(recipients.is_empty());
    }
}
