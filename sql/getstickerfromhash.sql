SELECT fileid
  FROM StickerLogs
 WHERE hash = $1
 LIMIT 1
