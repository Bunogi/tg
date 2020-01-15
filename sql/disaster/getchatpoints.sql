SELECT points, coalesce(userid, 0)
  FROM DisasterStatus
 WHERE chatid = $1
 GROUP BY userid, points
 ORDER BY points DESC
