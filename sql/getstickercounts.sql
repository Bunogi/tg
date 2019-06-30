SELECT fileid, COUNT(*) AS uses
  FROM StickerLogs
 GROUP BY fileid
 ORDER BY uses DESC
 LIMIT 10
