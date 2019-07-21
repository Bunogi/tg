SELECT points, IFNULL(userid, 0)
  FROM DisasterStatus
 WHERE chatid = ?
 GROUP BY userid
 ORDER BY points DESC
