SELECT userid, COUNT(message) AS messages
  FROM MessageLogs
 WHERE chatid = $1
 GROUP BY userid
 ORDER BY messages DESC
