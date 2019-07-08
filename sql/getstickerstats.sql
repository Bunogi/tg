SELECT COUNT(*) AS totalStickers,
       COUNT(DISTINCT packname) AS packs
  FROM StickerLogs
 WHERE chatid = ? AND instant > ?
