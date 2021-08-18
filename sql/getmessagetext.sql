SELECT message, userid, instant
  FROM MessageLogs
 WHERE chatid = $1
 ORDER BY instant ASC
