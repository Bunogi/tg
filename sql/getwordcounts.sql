--Splits every message into words in messagelogs, carry chatid all the way
WITH RECURSIVE split(lword, rest, chatid) AS (
  SELECT '', message || ' ' , chatid FROM MessageLogs
   UNION ALL
  SELECT SUBSTR(rest, 0, INSTR(rest, ' ')),
         SUBSTR(rest, INSTR(rest, ' ') + 1),
         chatid
    FROM split
   WHERE rest <> '')

SELECT LOWER(lword) as word, COUNT(*) AS uses
  FROM split
 WHERE word <> '' AND chatid = $1
 GROUP BY word
 ORDER BY uses DESC
 LIMIT $2
