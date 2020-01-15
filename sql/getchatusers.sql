SELECT COALESCE(username, firstname || ' ' || COALESCE(lastName, '')) AS name
  FROM LastUserData
 WHERE chatid = $1
 ORDER BY name
