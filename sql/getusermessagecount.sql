SELECT COUNT(message) AS messages
  FROM MessageLogs
 WHERE chatid = ? AND userid = ?
