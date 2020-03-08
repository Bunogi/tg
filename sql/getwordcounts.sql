WITH words AS (
  SELECT regexp_split_to_table(message, E'\\s+') AS word
    FROM MessageLogs
   WHERE chatid = $1
), lowerwords AS (
  SELECT LOWER(word) AS word
    FROM words
) SELECT word, COUNT(*) AS uses
    FROM lowerwords
   GROUP BY word
   ORDER BY uses DESC
   LIMIT $2
