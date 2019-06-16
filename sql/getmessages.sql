SELECT userid, COUNT(message) as messages
  FROM MessageLogs
 WHERE chatid = ?
 GROUP BY userid
 ORDER BY messages DESC
