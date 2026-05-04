CREATE TABLE IF NOT EXISTS nameplates (
    appid       TEXT NOT NULL,
    nameplate   TEXT NOT NULL,
    mailbox_id  TEXT NOT NULL,
    sides       TEXT[] NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (appid, nameplate)
);

CREATE TABLE IF NOT EXISTS mailboxes (
    appid          TEXT NOT NULL,
    mailbox_id     TEXT NOT NULL,
    claimed_sides  TEXT[] NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (appid, mailbox_id)
);

CREATE TABLE IF NOT EXISTS mailbox_messages (
    id          BIGSERIAL PRIMARY KEY,
    appid       TEXT NOT NULL,
    mailbox_id  TEXT NOT NULL,
    side        TEXT NOT NULL,
    phase       TEXT NOT NULL,
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS mailbox_messages_lookup_idx
    ON mailbox_messages (appid, mailbox_id, id);
