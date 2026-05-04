use crate::{
    protocol::{ClientMessage, NameplateEntry, ServerMessage, Welcome},
    state::{ConnectionId, DynStore, Tx},
};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub async fn run(listen: SocketAddr, store: DynStore) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    info!(%listen, "mailbox listening");
    serve(listener, store).await
}

pub async fn serve(listener: TcpListener, store: DynStore) -> Result<()> {
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "accept failed");
                continue;
            }
        };
        let store = store.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(sock, peer, store).await {
                debug!(error = %e, %peer, "connection ended");
            }
        });
    }
}

async fn handle(sock: TcpStream, peer: SocketAddr, store: DynStore) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(sock).await?;
    let conn_id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    debug!(%peer, conn_id, "connection accepted");
    let (mut sink, mut stream) = ws.split();

    let (tx, mut rx): (Tx, mpsc::UnboundedReceiver<ServerMessage>) = mpsc::unbounded_channel();

    tx.send(ServerMessage::Welcome { welcome: Welcome::default() }).ok();

    let mut appid: Option<String> = None;
    let mut side: Option<String> = None;
    let mut claimed_nameplates: Vec<String> = Vec::new();
    let mut open_mailboxes: Vec<String> = Vec::new();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "serialize");
                    continue;
                }
            };
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let outcome: Result<()> = async {
        while let Some(frame) = stream.next().await {
            let frame = frame?;
            let text = match frame {
                Message::Text(t) => t,
                Message::Binary(_) => {
                    send_error(&tx, "binary frames not supported", serde_json::Value::Null);
                    continue;
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                Message::Close(_) => break,
            };
            let raw: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    send_error(&tx, &format!("invalid JSON: {e}"), serde_json::Value::Null);
                    continue;
                }
            };
            let msg: ClientMessage = match serde_json::from_value(raw.clone()) {
                Ok(m) => m,
                Err(e) => {
                    send_error(&tx, &format!("unknown command: {e}"), raw);
                    continue;
                }
            };
            tx.send(ServerMessage::Ack).ok();

            handle_message(
                msg,
                store.as_ref(),
                &tx,
                conn_id,
                &mut appid,
                &mut side,
                &mut claimed_nameplates,
                &mut open_mailboxes,
            )
            .await;
        }
        Ok(())
    }
    .await;

    if let (Some(a), Some(s)) = (appid.as_deref(), side.as_deref()) {
        if let Err(e) = store
            .drop_connection(a, s, &claimed_nameplates, &open_mailboxes, conn_id)
            .await
        {
            error!(error = %e, "drop_connection cleanup failed");
        }
    }

    drop(tx);
    let _ = writer.await;
    outcome
}

async fn handle_message(
    msg: ClientMessage,
    store: &dyn crate::state::Store,
    tx: &Tx,
    conn_id: ConnectionId,
    appid: &mut Option<String>,
    side: &mut Option<String>,
    claimed_nameplates: &mut Vec<String>,
    open_mailboxes: &mut Vec<String>,
) {
    match msg {
        ClientMessage::SubmitPermission(_) => {}
        ClientMessage::Bind { appid: a, side: s } => {
            *appid = Some(a);
            *side = Some(s);
        }
        ClientMessage::Ping { ping } => {
            tx.send(ServerMessage::Pong { pong: ping }).ok();
        }
        ClientMessage::Allocate => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            match store.allocate_nameplate(a, s).await {
                Ok(nameplate) => {
                    claimed_nameplates.push(nameplate.clone());
                    tx.send(ServerMessage::Allocated { nameplate }).ok();
                }
                Err(e) => store_error(tx, "allocate", &e),
            }
        }
        ClientMessage::Claim { nameplate } => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            match store.claim_nameplate(a, &nameplate, s).await {
                Ok(Some(mailbox)) => {
                    if !claimed_nameplates.contains(&nameplate) {
                        claimed_nameplates.push(nameplate);
                    }
                    tx.send(ServerMessage::Claimed { mailbox }).ok();
                }
                Ok(None) => send_error(
                    tx,
                    "unknown nameplate",
                    serde_json::json!({"nameplate": nameplate}),
                ),
                Err(e) => store_error(tx, "claim", &e),
            }
        }
        ClientMessage::Release { nameplate } => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            let np = match nameplate {
                Some(n) => n,
                None => {
                    let Some(n) = claimed_nameplates.last().cloned() else {
                        return send_error(tx, "no nameplate to release", serde_json::Value::Null);
                    };
                    n
                }
            };
            match store.release_nameplate(a, &np, s).await {
                Ok(()) => {
                    claimed_nameplates.retain(|x| x != &np);
                    tx.send(ServerMessage::Released).ok();
                }
                Err(e) => store_error(tx, "release", &e),
            }
        }
        ClientMessage::List => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            match store.list_nameplates(a).await {
                Ok(ids) => {
                    let nameplates = ids.into_iter().map(|id| NameplateEntry { id }).collect();
                    tx.send(ServerMessage::Nameplates { nameplates }).ok();
                }
                Err(e) => store_error(tx, "list", &e),
            }
        }
        ClientMessage::Open { mailbox } => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            match store.open_mailbox(a, &mailbox, s, conn_id, tx.clone()).await {
                Ok(history) => {
                    if !open_mailboxes.contains(&mailbox) {
                        open_mailboxes.push(mailbox);
                    }
                    for m in history {
                        tx.send(ServerMessage::Message {
                            side: m.side,
                            phase: m.phase,
                            body: m.body,
                            id: None,
                        })
                        .ok();
                    }
                }
                Err(e) => store_error(tx, "open", &e),
            }
        }
        ClientMessage::Add { phase, body } => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            let Some(mb) = open_mailboxes.last() else {
                return send_error(tx, "must open a mailbox before add", serde_json::Value::Null);
            };
            match store.add_message(a, mb, s, &phase, &body).await {
                Ok(recipients) => {
                    for r in recipients {
                        r.send(ServerMessage::Message {
                            side: s.to_string(),
                            phase: phase.clone(),
                            body: body.clone(),
                            id: None,
                        })
                        .ok();
                    }
                }
                Err(e) => store_error(tx, "add", &e),
            }
        }
        ClientMessage::Close { mailbox, mood: _ } => {
            let Some(a) = appid.as_deref() else { return must_bind(tx) };
            let Some(s) = side.as_deref() else { return must_bind(tx) };
            let mb = match mailbox {
                Some(m) => m,
                None => {
                    let Some(m) = open_mailboxes.last().cloned() else {
                        return send_error(tx, "no mailbox to close", serde_json::Value::Null);
                    };
                    m
                }
            };
            match store.close_mailbox(a, &mb, s, conn_id).await {
                Ok(()) => {
                    open_mailboxes.retain(|x| x != &mb);
                    tx.send(ServerMessage::Closed).ok();
                }
                Err(e) => store_error(tx, "close", &e),
            }
        }
    }
}

fn must_bind(tx: &Tx) {
    send_error(tx, "must bind first", serde_json::Value::Null);
}

fn send_error(tx: &Tx, error: &str, orig: serde_json::Value) {
    tx.send(ServerMessage::Error {
        error: error.to_string(),
        orig,
    })
    .ok();
}

fn store_error(tx: &Tx, op: &str, err: &crate::state::StoreError) {
    error!(operation = %op, error = %err, "store error");
    send_error(tx, "internal store error", serde_json::Value::Null);
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::state::InMemoryStore;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
    };

    type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    async fn spawn_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let store = InMemoryStore::new();
        tokio::spawn(async move {
            let _ = serve(listener, store).await;
        });
        addr
    }

    async fn connect(addr: SocketAddr) -> Client {
        let url = format!("ws://{addr}");
        let (ws, _) = connect_async(url).await.unwrap();
        ws
    }

    async fn recv(ws: &mut Client) -> Value {
        loop {
            let frame = ws.next().await.expect("disconnected").unwrap();
            if let WsMessage::Text(t) = frame {
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    async fn recv_until(ws: &mut Client, ty: &str) -> Value {
        loop {
            let v = recv(ws).await;
            if v["type"] == ty {
                return v;
            }
        }
    }

    async fn send(ws: &mut Client, v: Value) {
        ws.send(WsMessage::Text(v.to_string().into())).await.unwrap();
    }

    #[tokio::test]
    async fn full_session_routes_messages_between_two_sides() {
        let addr = spawn_server().await;

        let mut alice = connect(addr).await;
        let mut bob = connect(addr).await;

        assert_eq!(recv(&mut alice).await["type"], "welcome");
        assert_eq!(recv(&mut bob).await["type"], "welcome");

        send(&mut alice, json!({"type": "bind", "appid": "test", "side": "alice"})).await;
        assert_eq!(recv(&mut alice).await["type"], "ack");
        send(&mut bob, json!({"type": "bind", "appid": "test", "side": "bob"})).await;
        assert_eq!(recv(&mut bob).await["type"], "ack");

        send(&mut alice, json!({"type": "allocate"})).await;
        let allocated = recv_until(&mut alice, "allocated").await;
        let nameplate = allocated["nameplate"].as_str().unwrap().to_string();

        send(&mut bob, json!({"type": "claim", "nameplate": nameplate})).await;
        let claimed = recv_until(&mut bob, "claimed").await;
        let mailbox = claimed["mailbox"].as_str().unwrap().to_string();

        send(&mut alice, json!({"type": "claim", "nameplate": nameplate})).await;
        let claimed_a = recv_until(&mut alice, "claimed").await;
        assert_eq!(claimed_a["mailbox"].as_str().unwrap(), mailbox);

        send(&mut alice, json!({"type": "open", "mailbox": mailbox})).await;
        send(&mut bob, json!({"type": "open", "mailbox": mailbox})).await;
        let _ = recv_until(&mut alice, "ack").await;
        let _ = recv_until(&mut bob, "ack").await;

        // Alice sends; both should receive (Alice gets her own echo).
        send(
            &mut alice,
            json!({"type": "add", "phase": "pake", "body": "deadbeef"}),
        )
        .await;
        let bob_msg = recv_until(&mut bob, "message").await;
        assert_eq!(bob_msg["side"], "alice");
        assert_eq!(bob_msg["phase"], "pake");
        assert_eq!(bob_msg["body"], "deadbeef");
        // Drain Alice's echo of her own send before reading Bob's reply.
        let alice_echo = recv_until(&mut alice, "message").await;
        assert_eq!(alice_echo["side"], "alice");

        // Bob replies; Alice should receive it.
        send(
            &mut bob,
            json!({"type": "add", "phase": "version", "body": "cafe"}),
        )
        .await;
        let alice_msg = recv_until(&mut alice, "message").await;
        assert_eq!(alice_msg["side"], "bob");
        assert_eq!(alice_msg["phase"], "version");
    }

    #[tokio::test]
    async fn allocate_before_bind_returns_error() {
        let addr = spawn_server().await;
        let mut ws = connect(addr).await;
        assert_eq!(recv(&mut ws).await["type"], "welcome");

        send(&mut ws, json!({"type": "allocate"})).await;
        // ack still comes back
        assert_eq!(recv(&mut ws).await["type"], "ack");
        let err = recv(&mut ws).await;
        assert_eq!(err["type"], "error");
        assert_eq!(err["error"], "must bind first");
    }

    #[tokio::test]
    async fn claim_unknown_nameplate_returns_error() {
        let addr = spawn_server().await;
        let mut ws = connect(addr).await;
        assert_eq!(recv(&mut ws).await["type"], "welcome");

        send(&mut ws, json!({"type": "bind", "appid": "test", "side": "x"})).await;
        assert_eq!(recv(&mut ws).await["type"], "ack");

        send(&mut ws, json!({"type": "claim", "nameplate": "9999"})).await;
        let _ = recv_until(&mut ws, "ack").await;
        let err = recv(&mut ws).await;
        assert_eq!(err["type"], "error");
        assert_eq!(err["error"], "unknown nameplate");
    }

    #[tokio::test]
    async fn ping_returns_pong() {
        let addr = spawn_server().await;
        let mut ws = connect(addr).await;
        assert_eq!(recv(&mut ws).await["type"], "welcome");
        send(&mut ws, json!({"type": "ping", "ping": 7})).await;
        let _ = recv_until(&mut ws, "ack").await;
        let pong = recv(&mut ws).await;
        assert_eq!(pong["type"], "pong");
        assert_eq!(pong["pong"], 7);
    }

    #[tokio::test]
    async fn late_opener_replays_existing_messages() {
        let addr = spawn_server().await;
        let mut alice = connect(addr).await;
        recv(&mut alice).await;
        send(&mut alice, json!({"type": "bind", "appid": "t", "side": "alice"})).await;
        recv(&mut alice).await;
        send(&mut alice, json!({"type": "allocate"})).await;
        let np = recv_until(&mut alice, "allocated").await["nameplate"]
            .as_str()
            .unwrap()
            .to_string();
        send(&mut alice, json!({"type": "claim", "nameplate": np})).await;
        let mb = recv_until(&mut alice, "claimed").await["mailbox"]
            .as_str()
            .unwrap()
            .to_string();
        send(&mut alice, json!({"type": "open", "mailbox": mb})).await;
        recv_until(&mut alice, "ack").await;
        send(
            &mut alice,
            json!({"type": "add", "phase": "early", "body": "0102"}),
        )
        .await;
        recv_until(&mut alice, "ack").await;

        // Bob joins late — should see the existing message on open.
        let mut bob = connect(addr).await;
        recv(&mut bob).await; // welcome
        send(&mut bob, json!({"type": "bind", "appid": "t", "side": "bob"})).await;
        recv(&mut bob).await;
        send(&mut bob, json!({"type": "claim", "nameplate": np})).await;
        recv_until(&mut bob, "claimed").await;
        send(&mut bob, json!({"type": "open", "mailbox": mb})).await;
        let replayed = recv_until(&mut bob, "message").await;
        assert_eq!(replayed["phase"], "early");
        assert_eq!(replayed["body"], "0102");
    }
}
