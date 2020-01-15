INSERT INTO DisasterStatus(chatid, userid, points)
VALUES ($1, $2, 1)
       ON CONFLICT(chatid, userid)
       DO UPDATE SET points = DisasterStatus.points + 1
