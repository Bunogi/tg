SELECT COUNT(message), MIN(instant)
  FROM MessageLogs
 WHERE chatid = $1
