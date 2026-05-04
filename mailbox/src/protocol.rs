use serde::{Deserialize, Serialize};

#[allow(dead_code)] // protocol surface; some fields are accepted but not acted on
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    SubmitPermission(SubmitPermission),
    Bind {
        appid: String,
        side: String,
    },
    List,
    Allocate,
    Claim {
        nameplate: String,
    },
    Release {
        nameplate: Option<String>,
    },
    Open {
        mailbox: String,
    },
    Add {
        phase: String,
        body: String,
    },
    Close {
        mailbox: Option<String>,
        mood: Option<String>,
    },
    Ping {
        ping: u64,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum SubmitPermission {
    Hashcash { stamp: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMessage {
    Welcome {
        welcome: Welcome,
    },
    Ack,
    Allocated {
        nameplate: String,
    },
    Claimed {
        mailbox: String,
    },
    Released,
    Nameplates {
        nameplates: Vec<NameplateEntry>,
    },
    Message {
        side: String,
        phase: String,
        body: String,
        id: Option<String>,
    },
    Closed,
    Pong {
        pong: u64,
    },
    Error {
        error: String,
        orig: serde_json::Value,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Welcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NameplateEntry {
    pub id: String,
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn parse_bind() {
        let v = json!({"type": "bind", "appid": "app", "side": "abc123", "id": null});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Bind { appid, side } => {
                assert_eq!(appid, "app");
                assert_eq!(side, "abc123");
            },
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_allocate() {
        let v = json!({"type": "allocate", "id": null});
        let msg: ClientMessage = from_value(v).unwrap();
        assert!(matches!(msg, ClientMessage::Allocate));
    }

    #[test]
    fn parse_add_with_hex_body() {
        let v = json!({"type": "add", "phase": "pake", "body": "deadbeef"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Add { phase, body } => {
                assert_eq!(phase, "pake");
                assert_eq!(body, "deadbeef");
            },
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_close_with_mood() {
        let v = json!({"type": "close", "mailbox": "mb1", "mood": "happy"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Close { mailbox, mood } => {
                assert_eq!(mailbox.as_deref(), Some("mb1"));
                assert_eq!(mood.as_deref(), Some("happy"));
            },
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn serialize_welcome() {
        let w = ServerMessage::Welcome {
            welcome: Welcome {
                motd: Some("hi".into()),
            },
        };
        assert_eq!(
            to_value(&w).unwrap(),
            json!({"type": "welcome", "welcome": {"motd": "hi"}})
        );
    }

    #[test]
    fn serialize_ack() {
        assert_eq!(
            to_value(ServerMessage::Ack).unwrap(),
            json!({"type": "ack"})
        );
    }

    #[test]
    fn serialize_allocated() {
        let m = ServerMessage::Allocated {
            nameplate: "4".into(),
        };
        assert_eq!(
            to_value(&m).unwrap(),
            json!({"type": "allocated", "nameplate": "4"})
        );
    }

    #[test]
    fn serialize_message() {
        let m = ServerMessage::Message {
            side: "abc".into(),
            phase: "pake".into(),
            body: "deadbeef".into(),
            id: None,
        };
        assert_eq!(
            to_value(&m).unwrap(),
            json!({"type": "message", "side": "abc", "phase": "pake", "body": "deadbeef", "id": null})
        );
    }

    #[test]
    fn serialize_nameplates() {
        let m = ServerMessage::Nameplates {
            nameplates: vec![
                NameplateEntry { id: "4".into() },
                NameplateEntry { id: "7".into() },
            ],
        };
        assert_eq!(
            to_value(&m).unwrap(),
            json!({"type": "nameplates", "nameplates": [{"id": "4"}, {"id": "7"}]})
        );
    }

    #[test]
    fn parse_release_with_explicit_nameplate() {
        let v = json!({"type": "release", "nameplate": "4"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Release { nameplate } => assert_eq!(nameplate.as_deref(), Some("4")),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_release_without_nameplate() {
        let v = json!({"type": "release"});
        let msg: ClientMessage = from_value(v).unwrap();
        assert!(matches!(msg, ClientMessage::Release { nameplate: None }));
    }

    #[test]
    fn parse_close_without_optional_fields() {
        let v = json!({"type": "close"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Close { mailbox, mood } => {
                assert!(mailbox.is_none());
                assert!(mood.is_none());
            },
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_open() {
        let v = json!({"type": "open", "mailbox": "mb-1"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::Open { mailbox } => assert_eq!(mailbox, "mb-1"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_ping() {
        let v = json!({"type": "ping", "ping": 42});
        let msg: ClientMessage = from_value(v).unwrap();
        assert!(matches!(msg, ClientMessage::Ping { ping: 42 }));
    }

    #[test]
    fn parse_submit_permission_hashcash() {
        let v = json!({"type": "submit-permission", "method": "hashcash", "stamp": "xyz"});
        let msg: ClientMessage = from_value(v).unwrap();
        match msg {
            ClientMessage::SubmitPermission(SubmitPermission::Hashcash { stamp }) => {
                assert_eq!(stamp, "xyz");
            },
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn serialize_pong() {
        let m = ServerMessage::Pong { pong: 99 };
        assert_eq!(to_value(&m).unwrap(), json!({"type": "pong", "pong": 99}));
    }

    #[test]
    fn serialize_error() {
        let m = ServerMessage::Error {
            error: "boom".into(),
            orig: json!({"type": "claim", "nameplate": "x"}),
        };
        assert_eq!(
            to_value(&m).unwrap(),
            json!({
                "type": "error",
                "error": "boom",
                "orig": {"type": "claim", "nameplate": "x"}
            })
        );
    }

    #[test]
    fn serialize_claimed_and_released() {
        assert_eq!(
            to_value(ServerMessage::Claimed {
                mailbox: "mb-1".into()
            })
            .unwrap(),
            json!({"type": "claimed", "mailbox": "mb-1"})
        );
        assert_eq!(
            to_value(ServerMessage::Released).unwrap(),
            json!({"type": "released"})
        );
        assert_eq!(
            to_value(ServerMessage::Closed).unwrap(),
            json!({"type": "closed"})
        );
    }
}
