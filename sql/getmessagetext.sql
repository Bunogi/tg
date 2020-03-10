-- Select the message in lowercase to improve the user experience when using
-- the simulate command with a seed.
SELECT LOWER(message), userid, instant
  FROM MessageLogs
 WHERE chatid = $1
 ORDER BY instant ASC
