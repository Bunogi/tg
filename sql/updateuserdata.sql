INSERT INTO LastUserData(id, chatid, firstname, lastname, username)
VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(id, chatid) DO
UPDATE SET firstname = ?3, lastname = ?4, username = ?5
