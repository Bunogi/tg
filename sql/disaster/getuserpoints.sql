SELECT IFNULL(points, 0)
  FROM DisasterStatus
 WHERE chatid = ? AND userid = ?
