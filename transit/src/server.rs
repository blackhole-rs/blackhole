use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

const HANDSHAKE_PREFIX: &str = "please relay ";
const MAX_HANDSHAKE_LEN: usize = 256;

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<TcpStream>>>>;

pub async fn run(listen: SocketAddr, wait_timeout: Duration) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    info!(%listen, "transit listening");
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "accept failed");
                continue;
            }
        };
        let pending = pending.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(sock, peer, pending, wait_timeout).await {
                debug!(error = %e, %peer, "connection ended");
            }
        });
    }
}

async fn handle(
    sock: TcpStream,
    peer: SocketAddr,
    pending: Pending,
    wait_timeout: Duration,
) -> Result<()> {
    let _ = sock.set_nodelay(true);
    let mut reader = BufReader::with_capacity(512, sock);
    let token = read_handshake(&mut reader).await.context("handshake")?;
    let sock = reader.into_inner();
    debug!(%peer, %token, "handshake received");

    // Try to claim a waiting peer or register ourselves.
    let waiting_peer_tx = {
        let mut map = pending.lock().unwrap();
        map.remove(&token)
    };

    match waiting_peer_tx {
        Some(peer_sender) => {
            // Hand our socket to the waiter; they'll write ok\n to both and start the relay.
            if peer_sender.send(sock).is_err() {
                debug!(%token, "waiting peer vanished before pairing");
            }
            Ok(())
        }
        None => {
            let (tx, rx) = oneshot::channel::<TcpStream>();
            {
                let mut map = pending.lock().unwrap();
                // If a duplicate token is already waiting, replace it (older waiter gets cancelled).
                map.insert(token.clone(), tx);
            }

            let peer_sock = match tokio::time::timeout(wait_timeout, rx).await {
                Ok(Ok(s)) => s,
                _ => {
                    pending.lock().unwrap().remove(&token);
                    return Err(anyhow!("timed out waiting for peer"));
                }
            };

            pair_and_relay(sock, peer_sock).await
        }
    }
}

async fn read_handshake(reader: &mut BufReader<TcpStream>) -> Result<String> {
    let mut line = String::new();
    let read_n = reader.read_line(&mut line).await.context("read line")?;
    if read_n == 0 {
        return Err(anyhow!("EOF before handshake"));
    }
    if line.len() > MAX_HANDSHAKE_LEN {
        return Err(anyhow!("handshake line too long"));
    }
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let rest = trimmed
        .strip_prefix(HANDSHAKE_PREFIX)
        .ok_or_else(|| anyhow!("expected handshake prefix"))?;
    // Form: "<hex-token>" or "<hex-token> for side <side-id>"
    let token = match rest.split_once(" for side ") {
        Some((tok, _side)) => tok,
        None => rest,
    };
    if token.is_empty() {
        return Err(anyhow!("empty token"));
    }
    Ok(token.to_string())
}

async fn pair_and_relay(mut a: TcpStream, mut b: TcpStream) -> Result<()> {
    let _ = a.write_all(b"ok\n").await;
    let _ = b.write_all(b"ok\n").await;
    let (a_to_b, b_to_a) = tokio::io::copy_bidirectional(&mut a, &mut b)
        .await
        .map(|(x, y)| (x, y))
        .unwrap_or((0, 0));
    debug!(a_to_b, b_to_a, "relay finished");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    async fn start_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(async move {
            loop {
                let (sock, peer) = listener.accept().await.unwrap();
                let pending = pending.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, peer, pending, Duration::from_secs(5)).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn pairs_two_clients_and_relays_bytes() {
        let addr = start_server().await;
        let token = "deadbeef";

        let mut a = TcpStream::connect(addr).await.unwrap();
        a.write_all(format!("please relay {token} for side aaa\n").as_bytes())
            .await
            .unwrap();

        let mut b = TcpStream::connect(addr).await.unwrap();
        b.write_all(format!("please relay {token} for side bbb\n").as_bytes())
            .await
            .unwrap();

        // Both should get "ok\n"
        let mut buf = [0u8; 3];
        a.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ok\n");
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ok\n");

        // a writes -> b reads
        a.write_all(b"hello").await.unwrap();
        let mut got = [0u8; 5];
        b.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"hello");

        // b writes -> a reads
        b.write_all(b"world!").await.unwrap();
        let mut got = [0u8; 6];
        a.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"world!");
    }

    #[tokio::test]
    async fn rejects_garbage_handshake() {
        let addr = start_server().await;
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(b"hello there\n").await.unwrap();
        // Server should drop the connection. Reading should give EOF.
        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf))
            .await
            .expect("server should close")
            .unwrap_or(0);
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn handshake_without_side_id_works() {
        let addr = start_server().await;
        let token = "cafebabe";

        let mut a = TcpStream::connect(addr).await.unwrap();
        a.write_all(format!("please relay {token}\n").as_bytes())
            .await
            .unwrap();
        let mut b = TcpStream::connect(addr).await.unwrap();
        b.write_all(format!("please relay {token}\n").as_bytes())
            .await
            .unwrap();

        let mut buf = [0u8; 3];
        a.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ok\n");
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ok\n");
    }

    async fn start_server_with_timeout(timeout: Duration) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(async move {
            loop {
                let (sock, peer) = listener.accept().await.unwrap();
                let pending = pending.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, peer, pending, timeout).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn waiter_times_out_when_peer_never_arrives() {
        let addr = start_server_with_timeout(Duration::from_millis(150)).await;
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(b"please relay loner for side a\n")
            .await
            .unwrap();
        // Server holds, then drops. EOF.
        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf))
            .await
            .expect("server should drop after timeout")
            .unwrap_or(0);
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn multiple_independent_pairs_dont_cross_streams() {
        let addr = start_server().await;

        let mut a1 = TcpStream::connect(addr).await.unwrap();
        a1.write_all(b"please relay token-aaa for side x\n")
            .await
            .unwrap();
        let mut b1 = TcpStream::connect(addr).await.unwrap();
        b1.write_all(b"please relay token-aaa for side y\n")
            .await
            .unwrap();

        let mut a2 = TcpStream::connect(addr).await.unwrap();
        a2.write_all(b"please relay token-bbb for side x\n")
            .await
            .unwrap();
        let mut b2 = TcpStream::connect(addr).await.unwrap();
        b2.write_all(b"please relay token-bbb for side y\n")
            .await
            .unwrap();

        // Drain ok\n on all four
        for s in [&mut a1, &mut b1, &mut a2, &mut b2] {
            let mut buf = [0u8; 3];
            s.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"ok\n");
        }

        // Pair 1 chats
        a1.write_all(b"hello-from-a1").await.unwrap();
        let mut got = [0u8; 13];
        b1.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"hello-from-a1");

        // Pair 2 chats independently
        b2.write_all(b"hello-from-b2").await.unwrap();
        let mut got = [0u8; 13];
        a2.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"hello-from-b2");
    }

    #[tokio::test]
    async fn peer_close_propagates_through_relay() {
        let addr = start_server().await;
        let mut a = TcpStream::connect(addr).await.unwrap();
        a.write_all(b"please relay token-c for side a\n")
            .await
            .unwrap();
        let mut b = TcpStream::connect(addr).await.unwrap();
        b.write_all(b"please relay token-c for side b\n")
            .await
            .unwrap();

        // Drain ok
        let mut buf = [0u8; 3];
        a.read_exact(&mut buf).await.unwrap();
        b.read_exact(&mut buf).await.unwrap();

        // a closes; b should see EOF.
        drop(a);
        let mut byte = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(2), b.read(&mut byte))
            .await
            .expect("relay should propagate EOF")
            .unwrap_or(0);
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn large_bidirectional_transfer_preserves_bytes() {
        let addr = start_server().await;
        let mut a = TcpStream::connect(addr).await.unwrap();
        a.write_all(b"please relay big for side a\n").await.unwrap();
        let mut b = TcpStream::connect(addr).await.unwrap();
        b.write_all(b"please relay big for side b\n").await.unwrap();

        let mut buf = [0u8; 3];
        a.read_exact(&mut buf).await.unwrap();
        b.read_exact(&mut buf).await.unwrap();

        let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();

        let payload_a = payload.clone();
        let send_a = tokio::spawn(async move {
            a.write_all(&payload_a).await.unwrap();
            a
        });

        let mut got = vec![0u8; payload.len()];
        b.read_exact(&mut got).await.unwrap();
        assert_eq!(got, payload);

        let _ = send_a.await.unwrap();
    }
}
