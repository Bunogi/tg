SELECT coalesce(points, 0)
  FROM DisasterStatus
 WHERE chatid = $1 AND userid = $2
