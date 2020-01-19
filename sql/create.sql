BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS MessageLogs (
  chatid BIGINT NOT NULL,
  userid BIGINT NOT NULL,
  msgid BIGINT NOT NULL,
  message TEXT NOT NULL,
  instant BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS EditLogs (
  chatid BIGINT NOT NULL,
  userid BIGINT NOT NULL,
  msgid BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS StickerLogs (
  userid BIGINT NOT NULL,
  chatid BIGINT NOT NULL,
  msgid BIGINT NOT NULL,
  fileid TEXT NOT NULL,
  packname TEXT,
  emoji TEXT,
  instant BIGINT NOT NULL,
  hash BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS LastUserData (
  id BIGINT,
  chatid BIGINT,
  firstname TEXT NOT NULL,
  lastname TEXT,
  username TEXT,
  PRIMARY KEY(id, chatid)
);

CREATE TABLE IF NOT EXISTS DisasterStatus (
  userid BIGINT,
  chatid BIGINT,
  points BIGINT,
  PRIMARY KEY(chatid, userid)
);

CREATE TABLE IF NOT EXISTS CommandNames (
  commandId BIGSERIAL PRIMARY KEY,
  command TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS CommandLogs (
  userid BIGINT NOT NULL,
  chatid BIGINT NOT NULL,
  command BIGINT NOT NULL REFERENCES CommandNames(commandId),
  logtime TIMESTAMP WITH TIME ZONE
);
COMMIT;
