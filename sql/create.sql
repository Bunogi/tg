CREATE TABLE IF NOT EXISTS MessageLogs (
  chatid INTEGER NOT NULL,
  userid INTEGER NOT NULL,
  msgid INTEGER NOT NULL,
  message TEXT NOT NULL,
  instant INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS EditLogs (
  chatid INTEGER NOT NULL,
  userid INTEGER NOT NULL,
  msgid INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS StickerLogs (
  userid INTEGER NOT NULL,
  chatid INTEGER NOT NULL,
  msgid INTEGER NOT NULL,
  fileid TEXT NOT NULL,
  packname TEXT,
  emoji TEXT,
  instant INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS LastUserData (
  id INTEGER,
  chatid INTEGER,
  firstname TEXT NOT NULL,
  lastname TEXT,
  username TEXT,
  PRIMARY KEY(id, chatid)
);

CREATE TABLE IF NOT EXISTS DisasterStatus (
  userid INTEGER,
  chatid INTEGER,
  points INTEGER,
  PRIMARY KEY(chatid, userid)
);
