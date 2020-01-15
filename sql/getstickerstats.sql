SELECT COUNT(*) AS totalStickers,
       COUNT(DISTINCT packname) AS packs
  FROM StickerLogs
 WHERE chatid = $1 AND instant > $2
