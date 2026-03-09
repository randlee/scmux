use crate::db;
use rusqlite::{params, types::Value as SqlValue, Connection, OptionalExtension};

pub struct WriteGuard(());

#[derive(Debug)]
pub enum WriteError {
    NotFound,
    Conflict(String),
    Validation(String),
    Forbidden(String),
    Internal(String),
}

impl WriteError {
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => "not found".to_string(),
            Self::Conflict(msg)
            | Self::Validation(msg)
            | Self::Forbidden(msg)
            | Self::Internal(msg) => msg.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewArmada {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ArmadaPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct NewFleet {
    pub armada_id: i64,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FleetPatch {
    pub armada_id: Option<i64>,
    pub name: Option<String>,
    pub color: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct NewFlotilla {
    pub fleet_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct FlotillaPatch {
    pub fleet_id: Option<i64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CrewMemberInput {
    pub member_id: String,
    pub role: String,
    pub ai_provider: String,
    pub model: String,
    pub startup_prompts_json: String,
}

#[derive(Debug, Clone)]
pub struct CrewVariantInput {
    pub host_id: i64,
    pub repo_url: Option<String>,
    pub branch_ref: Option<String>,
    pub root_path: String,
    pub config_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CrewPlacementInput {
    pub armada_id: i64,
    pub fleet_id: i64,
    pub flotilla_id: Option<i64>,
    pub alias_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewCrewBundle {
    pub crew_name: String,
    pub crew_ulid: String,
    pub members: Vec<CrewMemberInput>,
    pub variants: Vec<CrewVariantInput>,
    pub placement: CrewPlacementInput,
}

#[derive(Debug, Clone, Default)]
pub struct CrewBundlePatch {
    pub crew_ulid: Option<String>,
    pub members: Option<Vec<CrewMemberInput>>,
    pub variants: Option<Vec<CrewVariantInput>>,
}

#[derive(Debug, Clone)]
pub struct CloneCrewRequest {
    pub crew_name: String,
    pub crew_ulid: String,
    pub placement: CrewPlacementInput,
}

#[derive(Debug, Clone, Default)]
pub struct MoveCrewRefPatch {
    pub armada_id: i64,
    pub fleet_id: i64,
    pub flotilla_id: Option<i64>,
    pub alias_name: Option<Option<String>>,
}

pub fn create_session(conn: &Connection, new_session: &db::NewSession) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    validate_approved_project(&new_session.name, &new_session.config_json)?;
    db::create_session(&guard, conn, new_session).map_err(map_write_error)
}

pub fn patch_session(
    conn: &Connection,
    host_id: i64,
    name: &str,
    patch: &db::SessionPatch,
) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    if let Some(config_json) = patch.config_json.as_ref() {
        validate_approved_project(name, config_json)?;
    } else if patch.enabled == Some(true) {
        let current = db::get_session_for_host(conn, host_id, name)
            .map_err(map_write_error)?
            .ok_or(WriteError::NotFound)?;
        validate_approved_project(name, &current.config_json)?;
    }

    db::update_session(&guard, conn, host_id, name, patch).map_err(map_write_error)
}

pub fn delete_session(conn: &Connection, host_id: i64, name: &str) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    db::soft_delete_session(&guard, conn, host_id, name).map_err(map_write_error)
}

pub fn create_host(conn: &Connection, host: &db::NewHost) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    db::create_host(&guard, conn, host).map_err(map_write_error)
}

pub fn patch_host(
    conn: &Connection,
    host_id: i64,
    patch: &db::HostPatch,
) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    db::update_host(&guard, conn, host_id, patch).map_err(map_write_error)
}

pub fn delete_host(conn: &Connection, host_id: i64) -> Result<bool, WriteError> {
    let Some(row) = db::get_host(conn, host_id).map_err(map_write_error)? else {
        return Err(WriteError::NotFound);
    };
    if row.is_local {
        return Err(WriteError::Forbidden(
            "local host definition cannot be deleted".to_string(),
        ));
    }
    let guard = WriteGuard(());
    db::soft_delete_host(&guard, conn, host_id).map_err(map_write_error)
}

pub fn ensure_local_host(conn: &Connection) -> Result<i64, WriteError> {
    db::ensure_local_host(conn).map_err(|err| WriteError::Internal(err.to_string()))
}

pub fn create_armada(conn: &Connection, armada: &NewArmada) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    write_armada(&guard, conn, armada)
}

pub fn patch_armada(
    conn: &Connection,
    armada_id: i64,
    patch: &ArmadaPatch,
) -> Result<bool, WriteError> {
    let _guard = WriteGuard(());
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM armadas WHERE id = ?1",
            params![armada_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if exists.is_none() {
        return Ok(false);
    }

    let mut set_parts: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(name) = &patch.name {
        set_parts.push("name = ?");
        values.push(SqlValue::Text(name.clone()));
    }
    if let Some(description) = &patch.description {
        set_parts.push("description = ?");
        values.push(match description {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if set_parts.is_empty() {
        return Ok(true);
    }

    let mut sql = String::from("UPDATE armadas SET ");
    for (idx, part) in set_parts.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(part);
    }
    sql.push_str(" WHERE id = ?");
    values.push(SqlValue::Integer(armada_id));

    conn.execute(&sql, rusqlite::params_from_iter(values))
        .map_err(map_write_error)?;
    Ok(true)
}

pub fn create_fleet(conn: &Connection, fleet: &NewFleet) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    ensure_armada_exists(conn, fleet.armada_id)?;
    write_fleet(&guard, conn, fleet)
}

pub fn patch_fleet(
    conn: &Connection,
    fleet_id: i64,
    patch: &FleetPatch,
) -> Result<bool, WriteError> {
    let _guard = WriteGuard(());
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM fleets WHERE id = ?1",
            params![fleet_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if exists.is_none() {
        return Ok(false);
    }

    if let Some(armada_id) = patch.armada_id {
        ensure_armada_exists(conn, armada_id)?;
    }

    let mut set_parts: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(armada_id) = patch.armada_id {
        set_parts.push("armada_id = ?");
        values.push(SqlValue::Integer(armada_id));
    }
    if let Some(name) = &patch.name {
        set_parts.push("name = ?");
        values.push(SqlValue::Text(name.clone()));
    }
    if let Some(color) = &patch.color {
        set_parts.push("color = ?");
        values.push(match color {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if set_parts.is_empty() {
        return Ok(true);
    }

    let mut sql = String::from("UPDATE fleets SET ");
    for (idx, part) in set_parts.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(part);
    }
    sql.push_str(" WHERE id = ?");
    values.push(SqlValue::Integer(fleet_id));

    conn.execute(&sql, rusqlite::params_from_iter(values))
        .map_err(map_write_error)?;
    Ok(true)
}

pub fn create_flotilla(conn: &Connection, flotilla: &NewFlotilla) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    ensure_fleet_exists(conn, flotilla.fleet_id)?;
    write_flotilla(&guard, conn, flotilla)
}

pub fn patch_flotilla(
    conn: &Connection,
    flotilla_id: i64,
    patch: &FlotillaPatch,
) -> Result<bool, WriteError> {
    let _guard = WriteGuard(());
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM flotillas WHERE id = ?1",
            params![flotilla_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if exists.is_none() {
        return Ok(false);
    }

    if let Some(fleet_id) = patch.fleet_id {
        ensure_fleet_exists(conn, fleet_id)?;
    }

    let mut set_parts: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(fleet_id) = patch.fleet_id {
        set_parts.push("fleet_id = ?");
        values.push(SqlValue::Integer(fleet_id));
    }
    if let Some(name) = &patch.name {
        set_parts.push("name = ?");
        values.push(SqlValue::Text(name.clone()));
    }
    if set_parts.is_empty() {
        return Ok(true);
    }

    let mut sql = String::from("UPDATE flotillas SET ");
    for (idx, part) in set_parts.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(part);
    }
    sql.push_str(" WHERE id = ?");
    values.push(SqlValue::Integer(flotilla_id));

    conn.execute(&sql, rusqlite::params_from_iter(values))
        .map_err(map_write_error)?;
    Ok(true)
}

pub fn create_crew_bundle(conn: &Connection, bundle: &NewCrewBundle) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    validate_captain_count(&bundle.members)?;

    let tx = conn.unchecked_transaction().map_err(map_write_error)?;
    validate_placement(&tx, &bundle.placement)?;

    let crew_id = write_crew(&guard, &tx, &bundle.crew_name, &bundle.crew_ulid)?;

    insert_members(&guard, &tx, crew_id, &bundle.members)?;
    insert_variants(&guard, &tx, crew_id, &bundle.variants)?;

    tx.execute(
        "INSERT INTO crew_refs (crew_id, armada_id, fleet_id, flotilla_id, alias_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            crew_id,
            bundle.placement.armada_id,
            bundle.placement.fleet_id,
            bundle.placement.flotilla_id,
            bundle.placement.alias_name
        ],
    )
    .map_err(map_write_error)?;

    tx.commit().map_err(map_write_error)?;
    Ok(crew_id)
}

pub fn patch_crew_bundle(
    conn: &Connection,
    crew_id: i64,
    patch: &CrewBundlePatch,
) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    if let Some(members) = patch.members.as_ref() {
        validate_captain_count(members)?;
    }

    let tx = conn.unchecked_transaction().map_err(map_write_error)?;
    let exists: Option<i64> = tx
        .query_row(
            "SELECT id FROM crews WHERE id = ?1",
            params![crew_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if exists.is_none() {
        return Ok(false);
    }

    if let Some(crew_ulid) = patch.crew_ulid.as_ref() {
        tx.execute(
            "UPDATE crews SET crew_ulid = ?1 WHERE id = ?2",
            params![crew_ulid, crew_id],
        )
        .map_err(map_write_error)?;
    }

    if let Some(members) = patch.members.as_ref() {
        tx.execute(
            "DELETE FROM crew_members WHERE crew_id = ?1",
            params![crew_id],
        )
        .map_err(map_write_error)?;
        insert_members(&guard, &tx, crew_id, members)?;
    }

    if let Some(variants) = patch.variants.as_ref() {
        tx.execute(
            "DELETE FROM crew_variants WHERE crew_id = ?1",
            params![crew_id],
        )
        .map_err(map_write_error)?;
        insert_variants(&guard, &tx, crew_id, variants)?;
    }

    tx.commit().map_err(map_write_error)?;
    Ok(true)
}

pub fn clone_crew(
    conn: &Connection,
    source_crew_id: i64,
    request: &CloneCrewRequest,
) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    let tx = conn.unchecked_transaction().map_err(map_write_error)?;

    let source_exists: Option<i64> = tx
        .query_row(
            "SELECT id FROM crews WHERE id = ?1",
            params![source_crew_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if source_exists.is_none() {
        return Err(WriteError::NotFound);
    }

    validate_placement(&tx, &request.placement)?;

    let new_crew_id = write_crew(&guard, &tx, &request.crew_name, &request.crew_ulid)?;

    let source_members = {
        let mut stmt = tx
            .prepare(
                "SELECT member_id, role, ai_provider, model, startup_prompts_json
                 FROM crew_members
                 WHERE crew_id = ?1",
            )
            .map_err(map_write_error)?;
        let mapped = stmt
            .query_map(params![source_crew_id], |r| {
                Ok(CrewMemberInput {
                    member_id: r.get(0)?,
                    role: r.get(1)?,
                    ai_provider: r.get(2)?,
                    model: r.get(3)?,
                    startup_prompts_json: r.get(4)?,
                })
            })
            .map_err(map_write_error)?;
        mapped
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(map_write_error)?
    };
    for member in &source_members {
        write_crew_member(&guard, &tx, new_crew_id, member)?;
    }

    let source_variants = {
        let mut stmt = tx
            .prepare(
                "SELECT host_id, repo_url, branch_ref, root_path, config_json
                 FROM crew_variants
                 WHERE crew_id = ?1",
            )
            .map_err(map_write_error)?;
        let mapped = stmt
            .query_map(params![source_crew_id], |r| {
                Ok(CrewVariantInput {
                    host_id: r.get(0)?,
                    repo_url: r.get(1)?,
                    branch_ref: r.get(2)?,
                    root_path: r.get(3)?,
                    config_json: r.get(4)?,
                })
            })
            .map_err(map_write_error)?;
        mapped
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(map_write_error)?
    };
    for variant in &source_variants {
        write_crew_variant(&guard, &tx, new_crew_id, variant)?;
    }

    tx.execute(
        "INSERT INTO crew_refs (crew_id, armada_id, fleet_id, flotilla_id, alias_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            new_crew_id,
            request.placement.armada_id,
            request.placement.fleet_id,
            request.placement.flotilla_id,
            request.placement.alias_name
        ],
    )
    .map_err(map_write_error)?;

    tx.commit().map_err(map_write_error)?;
    Ok(new_crew_id)
}

pub fn move_crew_ref(
    conn: &Connection,
    ref_id: i64,
    patch: &MoveCrewRefPatch,
) -> Result<bool, WriteError> {
    let _guard = WriteGuard(());
    let tx = conn.unchecked_transaction().map_err(map_write_error)?;
    let exists: Option<i64> = tx
        .query_row(
            "SELECT id FROM crew_refs WHERE id = ?1",
            params![ref_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    if exists.is_none() {
        return Ok(false);
    }

    let placement = CrewPlacementInput {
        armada_id: patch.armada_id,
        fleet_id: patch.fleet_id,
        flotilla_id: patch.flotilla_id,
        alias_name: None,
    };
    validate_placement(&tx, &placement)?;

    if let Some(alias_name) = patch.alias_name.as_ref() {
        tx.execute(
            "UPDATE crew_refs
             SET armada_id = ?1, fleet_id = ?2, flotilla_id = ?3, alias_name = ?4
             WHERE id = ?5",
            params![
                patch.armada_id,
                patch.fleet_id,
                patch.flotilla_id,
                alias_name,
                ref_id
            ],
        )
        .map_err(map_write_error)?;
    } else {
        tx.execute(
            "UPDATE crew_refs
             SET armada_id = ?1, fleet_id = ?2, flotilla_id = ?3
             WHERE id = ?4",
            params![patch.armada_id, patch.fleet_id, patch.flotilla_id, ref_id],
        )
        .map_err(map_write_error)?;
    }

    tx.commit().map_err(map_write_error)?;
    Ok(true)
}

pub fn unlink_crew_ref(conn: &Connection, ref_id: i64) -> Result<bool, WriteError> {
    let tx = conn.unchecked_transaction().map_err(map_write_error)?;
    let crew_id: Option<i64> = tx
        .query_row(
            "SELECT crew_id FROM crew_refs WHERE id = ?1",
            params![ref_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;
    let Some(crew_id) = crew_id else {
        return Ok(false);
    };

    tx.execute("DELETE FROM crew_refs WHERE id = ?1", params![ref_id])
        .map_err(map_write_error)?;

    let remaining_refs: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM crew_refs WHERE crew_id = ?1",
            params![crew_id],
            |r| r.get(0),
        )
        .map_err(map_write_error)?;

    if remaining_refs == 0 {
        tx.execute("DELETE FROM crews WHERE id = ?1", params![crew_id])
            .map_err(map_write_error)?;
    }

    tx.commit().map_err(map_write_error)?;
    Ok(true)
}

fn insert_members(
    guard: &WriteGuard,
    tx: &rusqlite::Transaction<'_>,
    crew_id: i64,
    members: &[CrewMemberInput],
) -> Result<(), WriteError> {
    for member in members {
        write_crew_member(guard, tx, crew_id, member)?;
    }
    Ok(())
}

fn insert_variants(
    guard: &WriteGuard,
    tx: &rusqlite::Transaction<'_>,
    crew_id: i64,
    variants: &[CrewVariantInput],
) -> Result<(), WriteError> {
    for variant in variants {
        write_crew_variant(guard, tx, crew_id, variant)?;
    }
    Ok(())
}

fn write_armada(
    _guard: &WriteGuard,
    conn: &Connection,
    armada: &NewArmada,
) -> Result<i64, WriteError> {
    conn.execute(
        "INSERT INTO armadas (name, description) VALUES (?1, ?2)",
        params![armada.name, armada.description],
    )
    .map_err(map_write_error)?;
    Ok(conn.last_insert_rowid())
}

fn write_fleet(
    _guard: &WriteGuard,
    conn: &Connection,
    fleet: &NewFleet,
) -> Result<i64, WriteError> {
    conn.execute(
        "INSERT INTO fleets (armada_id, name, color) VALUES (?1, ?2, ?3)",
        params![fleet.armada_id, fleet.name, fleet.color],
    )
    .map_err(map_write_error)?;
    Ok(conn.last_insert_rowid())
}

fn write_flotilla(
    _guard: &WriteGuard,
    conn: &Connection,
    flotilla: &NewFlotilla,
) -> Result<i64, WriteError> {
    conn.execute(
        "INSERT INTO flotillas (fleet_id, name) VALUES (?1, ?2)",
        params![flotilla.fleet_id, flotilla.name],
    )
    .map_err(map_write_error)?;
    Ok(conn.last_insert_rowid())
}

fn write_crew(
    _guard: &WriteGuard,
    conn: &Connection,
    crew_name: &str,
    crew_ulid: &str,
) -> Result<i64, WriteError> {
    conn.execute(
        "INSERT INTO crews (crew_name, crew_ulid) VALUES (?1, ?2)",
        params![crew_name, crew_ulid],
    )
    .map_err(map_write_error)?;
    Ok(conn.last_insert_rowid())
}

fn write_crew_member(
    _guard: &WriteGuard,
    conn: &Connection,
    crew_id: i64,
    member: &CrewMemberInput,
) -> Result<(), WriteError> {
    conn.execute(
        "INSERT INTO crew_members (crew_id, member_id, role, ai_provider, model, startup_prompts_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            crew_id,
            member.member_id,
            member.role,
            member.ai_provider,
            member.model,
            member.startup_prompts_json
        ],
    )
    .map_err(map_write_error)?;
    Ok(())
}

fn write_crew_variant(
    _guard: &WriteGuard,
    conn: &Connection,
    crew_id: i64,
    variant: &CrewVariantInput,
) -> Result<(), WriteError> {
    conn.execute(
        "INSERT INTO crew_variants (crew_id, host_id, repo_url, branch_ref, root_path, config_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            crew_id,
            variant.host_id,
            variant.repo_url,
            variant.branch_ref,
            variant.root_path,
            variant.config_json
        ],
    )
    .map_err(map_write_error)?;
    Ok(())
}

fn validate_placement(
    tx: &rusqlite::Transaction<'_>,
    placement: &CrewPlacementInput,
) -> Result<(), WriteError> {
    let fleet_armada: Option<i64> = tx
        .query_row(
            "SELECT armada_id FROM fleets WHERE id = ?1",
            params![placement.fleet_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_write_error)?;

    let Some(fleet_armada) = fleet_armada else {
        return Err(WriteError::Validation(
            "fleet_id does not exist".to_string(),
        ));
    };

    if fleet_armada != placement.armada_id {
        return Err(WriteError::Validation(
            "fleet_id must belong to armada_id".to_string(),
        ));
    }

    if let Some(flotilla_id) = placement.flotilla_id {
        let flotilla_fleet: Option<i64> = tx
            .query_row(
                "SELECT fleet_id FROM flotillas WHERE id = ?1",
                params![flotilla_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_write_error)?;

        let Some(flotilla_fleet) = flotilla_fleet else {
            return Err(WriteError::Validation(
                "flotilla_id does not exist".to_string(),
            ));
        };

        if flotilla_fleet != placement.fleet_id {
            return Err(WriteError::Validation(
                "flotilla_id must belong to fleet_id".to_string(),
            ));
        }
    }

    Ok(())
}

fn ensure_armada_exists(conn: &Connection, armada_id: i64) -> Result<(), WriteError> {
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM armadas WHERE id = ?1",
            params![armada_id],
            |r| r.get(0),
        )
        .map_err(map_write_error)?;
    if !exists {
        return Err(WriteError::NotFound);
    }
    Ok(())
}

fn ensure_fleet_exists(conn: &Connection, fleet_id: i64) -> Result<(), WriteError> {
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM fleets WHERE id = ?1",
            params![fleet_id],
            |r| r.get(0),
        )
        .map_err(map_write_error)?;
    if !exists {
        return Err(WriteError::NotFound);
    }
    Ok(())
}

fn validate_captain_count(members: &[CrewMemberInput]) -> Result<(), WriteError> {
    let captain_count = members
        .iter()
        .filter(|member| member.role.trim().eq_ignore_ascii_case("captain"))
        .count();
    if captain_count != 1 {
        return Err(WriteError::Validation(
            "crew must have exactly one captain".to_string(),
        ));
    }
    Ok(())
}

fn validate_approved_project(session_name: &str, config_json: &str) -> Result<(), WriteError> {
    let value: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|err| WriteError::Validation(format!("invalid config_json JSON: {err}")))?;

    // DG-04 approval policy split:
    // - `enabled = 1` is enforced by the session row lifecycle and editor route semantics.
    // - This validator enforces structural approval requirements for writer-gate calls
    //   (`config_json.session_name` match and non-empty `config_json.panes[]`).
    let json_session_name = value
        .get("session_name")
        .and_then(|raw| raw.as_str())
        .ok_or_else(|| {
            WriteError::Validation("config_json.session_name is required".to_string())
        })?;
    if json_session_name != session_name {
        return Err(WriteError::Validation(
            "config_json.session_name must equal session name".to_string(),
        ));
    }

    let panes = value
        .get("panes")
        .and_then(|raw| raw.as_array())
        .ok_or_else(|| {
            WriteError::Validation(
                "config_json.panes[] is required for approved projects".to_string(),
            )
        })?;

    if panes.is_empty() {
        return Err(WriteError::Validation(
            "config_json.panes[] must contain at least one pane".to_string(),
        ));
    }

    Ok(())
}

fn map_write_error(err: impl std::fmt::Display) -> WriteError {
    let message = err.to_string();
    if message.contains("UNIQUE constraint failed") {
        return WriteError::Conflict(message);
    }
    if message.contains("FOREIGN KEY constraint failed") {
        return WriteError::Validation(message);
    }
    if message.contains("invalid") || message.contains("required") || message.contains("must") {
        return WriteError::Validation(message);
    }
    WriteError::Internal(message)
}
