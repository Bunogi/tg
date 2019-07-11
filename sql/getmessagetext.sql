SELECT message, userid, instant
  FROM MessageLogs
 WHERE chatid = ?
 ORDER BY instant ASC
