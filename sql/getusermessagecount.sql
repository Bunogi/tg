SELECT COUNT(message) AS messages
  FROM MessageLogs
 WHERE chatid = $1 AND userid = $2
