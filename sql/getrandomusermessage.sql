SELECT message, instant
  FROM MessageLogs
 WHERE chatid = $1 AND userid = $2
 ORDER BY RANDOM()
 LIMIT 1
