SELECT IFNULL(username, firstname || " " || IFNULL(lastName, "")) AS name
  FROM LastUserData
 WHERE chatid = ?
 ORDER BY name
