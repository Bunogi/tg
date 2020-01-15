SELECT userid, message
  FROM MessageLogs
 WHERE chatid = $1
