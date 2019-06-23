SELECT COUNT(*) AS totalStickers,
       COUNT(DISTINCT packname) AS packs,
       MIN(instant) AS earliest
  FROM StickerLogs
 WHERE chatid = ?
