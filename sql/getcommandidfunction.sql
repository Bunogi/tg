CREATE OR REPLACE FUNCTION getCommandId(cmd TEXT) RETURNS BIGINT AS $$
  DECLARE
    output_commandId BIGINT;
  BEGIN
    SELECT commandId
      INTO output_commandId
      FROM commandNames
     WHERE commandNames.command = cmd;
    IF output_commandId IS NULL THEN
      INSERT INTO CommandNames(command)
      VALUES(cmd)
      RETURNING CommandNames.CommandId INTO output_commandId;
    END IF;
    RETURN output_commandId;
  END;
$$ LANGUAGE PLPGSQL;
