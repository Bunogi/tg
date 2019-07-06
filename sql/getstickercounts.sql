SELECT fileid, COUNT(*) AS uses
  FROM StickerLogs
 WHERE chatid = ? AND instant > ?
 GROUP BY fileid
 ORDER BY uses DESC
 LIMIT 15
