SELECT fileid, COUNT(*) AS uses
  FROM StickerLogs
 WHERE chatid = $1 AND instant > $2
 GROUP BY hash
 ORDER BY uses DESC
 LIMIT 25
