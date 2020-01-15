SELECT id FROM LastUserData
 WHERE chatid = $1
   AND (username ILIKE '%' || $2 || '%'
        OR firstname || ' ' || lastname
            ILIKE '%' || $2  || '%')
 LIMIT 1
