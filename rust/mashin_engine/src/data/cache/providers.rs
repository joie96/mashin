//! User-scoped provider observations; callers own the read snapshot or atomic writer transaction.
//! Only observed selections are reconstructed; opening the cache never loads provider inventories.

use super::database::CacheError;
use crate::domain::identifiers::{ProviderInstanceId, UserId};
use crate::domain::providers::{ProviderInstance, ProviderManifest};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ProviderIconRole {
    Icon,
    Svg,
    Dark,
    Monochrome,
    Default,
}

impl ProviderIconRole {
    pub(super) const INLINE: [Self; 4] = [Self::Icon, Self::Svg, Self::Dark, Self::Monochrome];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Icon => "icon",
            Self::Svg => "icon_svg",
            Self::Dark => "icon_svg_dark",
            Self::Monochrome => "icon_svg_monochrome",
            Self::Default => "default",
        }
    }

    pub(super) fn value_mut(
        self,
        manifest: &mut ProviderManifest,
    ) -> Result<&mut Option<String>, CacheError> {
        match self {
            Self::Icon => Ok(&mut manifest.icon),
            Self::Svg => Ok(&mut manifest.icon_svg),
            Self::Dark => Ok(&mut manifest.icon_svg_dark),
            Self::Monochrome => Ok(&mut manifest.icon_svg_monochrome),
            Self::Default => Err(CacheError::InvalidMediaObservation(
                "default icon is not a manifest field",
            )),
        }
    }

    fn is_known(self, manifest: &ProviderManifest) -> bool {
        match self {
            Self::Icon => manifest.knowledge.icon,
            Self::Svg => manifest.knowledge.icon_svg,
            Self::Dark => manifest.knowledge.icon_svg_dark,
            Self::Monochrome => manifest.knowledge.icon_svg_monochrome,
            Self::Default => false,
        }
    }
}

#[derive(Clone)]
pub(super) struct StoredManifest {
    pub(super) manifest: ProviderManifest,
    pub(super) icons: Vec<ProviderIconRole>,
}

impl StoredManifest {
    // The image owner must publish these slots before this projection enters SQL.
    pub(super) fn from_published_manifest(
        mut manifest: ProviderManifest,
    ) -> Result<Self, CacheError> {
        let mut icons = Vec::new();
        for role in ProviderIconRole::INLINE {
            let value = role.value_mut(&mut manifest)?;
            if value
                .as_deref()
                .is_some_and(crate::domain::artwork::is_inline_source)
            {
                *value = None;
                icons.push(role);
            }
        }
        Ok(Self { manifest, icons })
    }
}

// Schema and confirmed server observations.
pub(super) fn initialize_provider_schema(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    transaction.execute_batch("CREATE TABLE provider_instance_reads (
        user_id TEXT PRIMARY KEY REFERENCES users(user_id),
        observed_at INTEGER NOT NULL CHECK(observed_at>=0)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE provider_instances (
        user_id TEXT NOT NULL REFERENCES provider_instance_reads(user_id) ON DELETE CASCADE,
        instance_id TEXT NOT NULL CHECK(length(instance_id)>0),
        available INTEGER NOT NULL CHECK(available IN (0,1)),
        position INTEGER NOT NULL CHECK(position>=0),
        PRIMARY KEY(user_id,instance_id)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE provider_manifest_reads (
        user_id TEXT PRIMARY KEY REFERENCES users(user_id),observed_at INTEGER NOT NULL CHECK(observed_at>=0)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE provider_manifests (
        user_id TEXT NOT NULL REFERENCES provider_manifest_reads(user_id) ON DELETE CASCADE,
        domain TEXT NOT NULL CHECK(length(domain)>0),metadata TEXT NOT NULL CHECK(json_type(metadata)='object'),
        position INTEGER NOT NULL CHECK(position>=0),PRIMARY KEY(user_id,domain)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE provider_manifest_icons (
        user_id TEXT NOT NULL,domain TEXT NOT NULL,
        role TEXT NOT NULL CHECK(role IN ('icon','icon_svg','icon_svg_dark','icon_svg_monochrome')),
        PRIMARY KEY(user_id,domain,role),FOREIGN KEY(user_id,domain) REFERENCES provider_manifests(user_id,domain) ON DELETE CASCADE
    ) STRICT, WITHOUT ROWID;")?;
    Ok(())
}

pub(super) fn write_instances(
    transaction: &Transaction<'_>,
    user: &UserId,
    instances: &[ProviderInstance],
    observed_at: i64,
) -> Result<(), CacheError> {
    let mut identities = HashSet::new();
    for instance in instances {
        if instance.instance_id.as_str().is_empty()
            || !identities.insert(instance.instance_id.as_str())
        {
            return Err(CacheError::InvalidMediaObservation(
                "provider instance identities must be nonempty and unique",
            ));
        }
    }
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user.as_str()],
    )?;
    transaction.execute("INSERT INTO provider_instance_reads(user_id,observed_at) VALUES(?1,?2)
        ON CONFLICT(user_id) DO UPDATE SET observed_at=excluded.observed_at WHERE observed_at IS NOT excluded.observed_at",
        params![user.as_str(),observed_at])?;
    let ids = serde_json::to_string(
        &instances
            .iter()
            .map(|instance| instance.instance_id.as_str())
            .collect::<Vec<_>>(),
    )?;
    transaction.execute("DELETE FROM provider_instances WHERE user_id=?1 AND instance_id NOT IN(SELECT value FROM json_each(?2))", params![user.as_str(),ids])?;
    for (position, instance) in instances.iter().enumerate() {
        transaction.execute("INSERT INTO provider_instances(user_id,instance_id,available,position) VALUES(?1,?2,?3,?4)
            ON CONFLICT(user_id,instance_id) DO UPDATE SET available=excluded.available,position=excluded.position
            WHERE available IS NOT excluded.available OR position IS NOT excluded.position",
            params![user.as_str(),instance.instance_id.as_str(),instance.available,position as i64])?;
    }
    Ok(())
}

pub(super) fn write_manifests(
    transaction: &Transaction<'_>,
    user: &UserId,
    manifests: &[StoredManifest],
    observed_at: i64,
) -> Result<(), CacheError> {
    let mut identities = HashSet::new();
    for stored in manifests {
        if stored.manifest.domain.is_empty() || !identities.insert(stored.manifest.domain.as_str())
        {
            return Err(CacheError::InvalidMediaObservation(
                "provider manifest domains must be nonempty and unique",
            ));
        }
        for value in [
            &stored.manifest.icon,
            &stored.manifest.icon_svg,
            &stored.manifest.icon_svg_dark,
            &stored.manifest.icon_svg_monochrome,
        ]
        .into_iter()
        .flatten()
        .chain(stored.manifest.icon_images.iter())
        {
            if crate::domain::artwork::is_inline_source(value) {
                return Err(CacheError::InvalidMediaObservation(
                    "provider icon bytes require an image file reference",
                ));
            }
        }
        if stored
            .icons
            .iter()
            .any(|role| !role.is_known(&stored.manifest))
        {
            return Err(CacheError::InvalidMediaObservation(
                "provider icon references require an observed manifest slot",
            ));
        }
    }
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user.as_str()],
    )?;
    transaction.execute("INSERT INTO provider_manifest_reads(user_id,observed_at) VALUES(?1,?2)
        ON CONFLICT(user_id) DO UPDATE SET observed_at=excluded.observed_at WHERE observed_at IS NOT excluded.observed_at",params![user.as_str(),observed_at])?;
    let domains = serde_json::to_string(
        &manifests
            .iter()
            .map(|stored| stored.manifest.domain.as_str())
            .collect::<Vec<_>>(),
    )?;
    transaction.execute("DELETE FROM provider_manifests WHERE user_id=?1 AND domain NOT IN(SELECT value FROM json_each(?2))",params![user.as_str(),domains])?;
    for (position, incoming) in manifests.iter().enumerate() {
        let previous = read_manifest(transaction, user, &incoming.manifest.domain)?;
        let previous_icons = previous
            .as_ref()
            .map(|stored| stored.icons.clone())
            .unwrap_or_default();
        let mut stored = previous.unwrap_or_else(|| StoredManifest {
            manifest: incoming.manifest.clone(),
            icons: Vec::new(),
        });
        stored
            .icons
            .retain(|role| !role.is_known(&incoming.manifest));
        stored.icons.extend(incoming.icons.iter().copied());
        stored.icons.sort_unstable();
        stored.icons.dedup();
        stored.manifest.merge_known(incoming.manifest.clone());
        let metadata = serde_json::to_string(&stored.manifest)?;
        transaction.execute("INSERT INTO provider_manifests(user_id,domain,metadata,position) VALUES(?1,?2,?3,?4)
            ON CONFLICT(user_id,domain) DO UPDATE SET metadata=excluded.metadata,position=excluded.position
            WHERE metadata IS NOT excluded.metadata OR position IS NOT excluded.position",params![user.as_str(),stored.manifest.domain,metadata,position as i64])?;
        if previous_icons != stored.icons {
            transaction.execute(
                "DELETE FROM provider_manifest_icons WHERE user_id=?1 AND domain=?2",
                params![user.as_str(), stored.manifest.domain],
            )?;
            for role in stored.icons {
                transaction.execute(
                    "INSERT INTO provider_manifest_icons(user_id,domain,role) VALUES(?1,?2,?3)",
                    params![user.as_str(), stored.manifest.domain, role.as_str()],
                )?;
            }
        }
    }
    Ok(())
}

// Missing coverage differs from a confirmed empty provider list.
pub(super) fn read_manifest_observation(
    transaction: &Transaction<'_>,
    user: &UserId,
) -> Result<Option<(Vec<StoredManifest>, i64)>, CacheError> {
    let Some(manifests) = read_manifests(transaction, user)? else {
        return Ok(None);
    };
    let observed_at = transaction.query_row(
        "SELECT observed_at FROM provider_manifest_reads WHERE user_id=?1",
        [user.as_str()],
        |row| row.get(0),
    )?;
    Ok(Some((manifests, observed_at)))
}

pub(super) fn read_instance_observation(
    transaction: &Transaction<'_>,
    user: &UserId,
) -> Result<Option<super::reads::CatalogReadObservation>, CacheError> {
    let Some(instances) = read_instances(transaction, user)? else {
        return Ok(None);
    };
    let observed_at = transaction.query_row(
        "SELECT observed_at FROM provider_instance_reads WHERE user_id=?1",
        [user.as_str()],
        |row| row.get(0),
    )?;
    Ok(Some(super::reads::CatalogReadObservation {
        value: crate::domain::catalog::CatalogValue::ProviderInstances(instances),
        observed_at,
    }))
}

pub(super) fn read_instances(
    transaction: &Transaction<'_>,
    user: &UserId,
) -> Result<Option<Vec<ProviderInstance>>, CacheError> {
    let observed = transaction
        .query_row(
            "SELECT observed_at FROM provider_instance_reads WHERE user_id=?1",
            [user.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if observed.is_none() {
        return Ok(None);
    }
    let mut statement = transaction.prepare(
        "SELECT instance_id,available FROM provider_instances WHERE user_id=?1 ORDER BY position",
    )?;
    let instances = statement
        .query_map([user.as_str()], |row| {
            Ok(ProviderInstance {
                instance_id: ProviderInstanceId(row.get(0)?),
                available: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(instances))
}

pub(super) fn read_manifests(
    transaction: &Transaction<'_>,
    user: &UserId,
) -> Result<Option<Vec<StoredManifest>>, CacheError> {
    if transaction
        .query_row(
            "SELECT observed_at FROM provider_manifest_reads WHERE user_id=?1",
            [user.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_none()
    {
        return Ok(None);
    }
    let mut statement = transaction
        .prepare("SELECT domain FROM provider_manifests WHERE user_id=?1 ORDER BY position")?;
    let domains = statement
        .query_map([user.as_str()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut manifests = Vec::new();
    for domain in domains {
        manifests.push(read_manifest(transaction, user, &domain)?.ok_or(
            CacheError::InvalidMediaObservation("provider manifest missing"),
        )?);
    }
    Ok(Some(manifests))
}

// Point reconstruction is also used to merge partial server observations without an inventory read.
fn read_manifest(
    transaction: &Transaction<'_>,
    user: &UserId,
    domain: &str,
) -> Result<Option<StoredManifest>, CacheError> {
    let metadata = transaction
        .query_row(
            "SELECT metadata FROM provider_manifests WHERE user_id=?1 AND domain=?2",
            params![user.as_str(), domain],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let manifest: ProviderManifest = serde_json::from_str(&metadata)?;
    if manifest.domain != domain {
        return Err(CacheError::InvalidMediaObservation(
            "provider manifest identity mismatch",
        ));
    }
    let mut statement = transaction.prepare(
        "SELECT role FROM provider_manifest_icons WHERE user_id=?1 AND domain=?2 ORDER BY role",
    )?;
    let roles = statement
        .query_map(params![user.as_str(), domain], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut icons = Vec::new();
    for role in roles {
        icons.push(
            ProviderIconRole::INLINE
                .into_iter()
                .find(|candidate| candidate.as_str() == role)
                .ok_or(CacheError::InvalidMediaObservation(
                    "unknown provider icon slot",
                ))?,
        );
    }
    icons.sort_unstable();
    Ok(Some(StoredManifest { manifest, icons }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_manifests_merge_partial_fields_and_keep_payloads_out_of_sql() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch("CREATE TABLE users(user_id TEXT PRIMARY KEY) STRICT, WITHOUT ROWID;")
            .unwrap();
        initialize_provider_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let stored = StoredManifest { manifest:serde_json::from_value(serde_json::json!({"domain":"test","name":"Original","icon_svg":null,"builtin":true,"icon_images":["default"]})).unwrap(),icons:vec![ProviderIconRole::Svg] };
        write_manifests(&transaction, &user, &[stored.clone()], 100).unwrap();
        let before = transaction.total_changes();
        write_manifests(&transaction, &user, &[stored], 100).unwrap();
        assert_eq!(transaction.total_changes(), before);
        let partial = StoredManifest {
            manifest: serde_json::from_value(serde_json::json!({"domain":"test","builtin":false}))
                .unwrap(),
            icons: vec![],
        };
        write_manifests(&transaction, &user, &[partial], 101).unwrap();
        let current = read_manifests(&transaction, &user)
            .unwrap()
            .unwrap()
            .remove(0);
        assert_eq!(current.manifest.name.as_deref(), Some("Original"));
        assert_eq!(current.manifest.builtin, Some(false));
        assert_eq!(current.icons, vec![ProviderIconRole::Svg]);
        let before = transaction.total_changes();
        let invalid = StoredManifest {
            manifest: serde_json::from_value(
                serde_json::json!({"domain":"test","icon_svg":"<svg/>"}),
            )
            .unwrap(),
            icons: vec![],
        };
        assert!(write_manifests(&transaction, &user, &[invalid], 102).is_err());
        assert_eq!(transaction.total_changes(), before);
        write_manifests(&transaction, &UserId("other".into()), &[], 102).unwrap();
        assert!(read_manifests(&transaction, &UserId("other".into()))
            .unwrap()
            .unwrap()
            .is_empty());
        assert_eq!(
            read_manifests(&transaction, &user).unwrap().unwrap().len(),
            1
        );
        let clear = StoredManifest {
            manifest: serde_json::from_value(serde_json::json!({"domain":"test","icon_svg":null}))
                .unwrap(),
            icons: vec![],
        };
        write_manifests(&transaction, &user, &[clear], 103).unwrap();
        assert!(read_manifests(&transaction, &user).unwrap().unwrap()[0]
            .icons
            .is_empty());
    }

    #[test]
    fn provider_instances_preserve_scope_order_empty_coverage_and_atomic_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.sqlite3");
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch("CREATE TABLE users(user_id TEXT PRIMARY KEY) STRICT, WITHOUT ROWID;")
            .unwrap();
        initialize_provider_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let instances = vec![
            ProviderInstance {
                instance_id: ProviderInstanceId("second".into()),
                available: false,
            },
            ProviderInstance {
                instance_id: ProviderInstanceId("first".into()),
                available: true,
            },
        ];
        write_instances(&transaction, &user, &instances, 100).unwrap();
        write_instances(&transaction, &other, &[], 100).unwrap();
        let before = transaction.total_changes();
        write_instances(&transaction, &user, &instances, 100).unwrap();
        assert_eq!(transaction.total_changes(), before);
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        assert!(write_instances(
            &transaction,
            &user,
            &[instances[0].clone(), instances[0].clone()],
            200
        )
        .is_err());
        transaction.rollback().unwrap();
        drop(connection);
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_instances(&transaction, &user).unwrap(),
            Some(instances)
        );
        assert_eq!(read_instances(&transaction, &other).unwrap(), Some(vec![]));
        assert_eq!(
            read_instances(&transaction, &UserId("unknown".into())).unwrap(),
            None
        );
        transaction.rollback().unwrap();
        drop(connection);
        directory.close().unwrap();
    }
}
