SELECT id FROM LastUserData
 WHERE chatid = ?
   AND (firstname   LIKE "%" || ?2 || "%"
        OR lastname LIKE "%" || ?2 || "%"
        OR username LIKE "%" || ?2 || "%")
 LIMIT 1
