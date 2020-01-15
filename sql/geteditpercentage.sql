SELECT MessageLogs.userid,
       edits.uniqueEdits * 1.0 / COUNT(*) * 100.0 AS percentage,
       edits.totalEdits AS totalEdits
  FROM MessageLogs
         JOIN (SELECT COUNT(DISTINCT EditLogs.msgid) AS uniqueEdits,
                      COUNT(EditLogs.msgid) AS totalEdits,
                      EditLogs.userid
                 FROM EditLogs
                WHERE EditLogs.chatid = $1
                GROUP BY EditLogs.userid
         ) AS edits ON MessageLogs.userid = edits.userid
 WHERE MessageLogs.chatid = $1
 GROUP BY MessageLogs.userid, edits.totalEdits, edits.uniqueEdits
 ORDER BY percentage DESC
