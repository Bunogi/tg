SELECT fileid, COUNT(*) AS uses
  FROM StickerLogs
 WHERE chatid = ? AND instant > ?
 GROUP BY hash
 ORDER BY uses DESC
 LIMIT 25
