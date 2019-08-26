SELECT userid, COUNT(message) AS messages
  FROM MessageLogs
 WHERE chatid = ?
 GROUP BY userid
 ORDER BY messages DESC
