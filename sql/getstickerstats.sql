SELECT COUNT(*) AS totalStickers,
       COUNT(DISTINCT packname) AS packs,
       IFNULL(MIN(instant), 0) AS earliest
  FROM StickerLogs
 WHERE chatid = ? AND instant > ?
