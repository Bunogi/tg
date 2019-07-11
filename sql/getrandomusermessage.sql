SELECT message, instant
  FROM MessageLogs
 WHERE chatid = ? AND userid = ?
 ORDER BY RANDOM()
 LIMIT 1
