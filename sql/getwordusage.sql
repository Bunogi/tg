WITH words AS (
  SELECT regexp_split_to_table(message, E'\\s+') AS word
    FROM MessageLogs
   WHERE chatid = $2
), lowerwords AS (
  SELECT LOWER(word) AS word
    FROM words
) SELECT word, COUNT(*) AS uses
  FROM lowerwords
 WHERE word = LOWER($1)
 GROUP BY word
