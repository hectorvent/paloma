//! Template groups, which analytics can report on together.

use super::{
    App, ArgGroup, Args, GroupUpdate, Result, Subcommand, field_id, integer_ids, limits, templates,
    validate,
};

#[derive(Debug, Subcommand)]
pub enum GroupCommand {
    /// Create a template group.
    Create(GroupCreate),
    /// Show a template group and its templates.
    Get {
        /// Template group id.
        id: String,
    },
    /// Change a template group's name or description, or add and remove templates.
    Update(GroupUpdateArgs),
    /// Delete a template group. Its templates stay.
    Delete {
        /// Template group id.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct GroupCreate {
    /// Group name, unique on the WABA (up to 256 characters).
    #[arg(long)]
    pub(super) name: String,
    /// Group description (up to 512 characters).
    #[arg(long)]
    pub(super) description: Option<String>,
    /// Template id; repeat it or separate ids with commas. A template can be in one group at
    /// a time.
    #[arg(
        long = "template",
        required = true,
        value_delimiter = ',',
        value_name = "ID,..."
    )]
    pub(super) templates: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("change")
        .required(true)
        .multiple(true)
        .args(["name", "description", "add", "remove"])
))]
pub struct GroupUpdateArgs {
    /// Template group id.
    pub(super) id: String,
    /// New name (up to 256 characters).
    #[arg(long)]
    pub(super) name: Option<String>,
    /// New description (up to 512 characters).
    #[arg(long)]
    pub(super) description: Option<String>,
    /// Template id to add; repeat it or separate ids with commas.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    pub(super) add: Vec<String>,
    /// Template id to remove; repeat it or separate ids with commas. Metrics it contributed
    /// stay with the group.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    pub(super) remove: Vec<String>,
}

pub(super) async fn group(app: &App, command: GroupCommand) -> Result<()> {
    let name_ok =
        |name: &str| validate::template_text(name, limits::MAX_TEMPLATE_GROUP_NAME, "--name");
    let description_ok = |description: &str| {
        validate::template_text(
            description,
            limits::MAX_TEMPLATE_GROUP_DESCRIPTION,
            "--description",
        )
    };
    match command {
        GroupCommand::Create(a) => {
            name_ok(&a.name)?;
            a.description.as_deref().map_or(Ok(()), description_ok)?;
            let ids = integer_ids(&a.templates)?;
            let waba = app.ctx()?.require_waba_id()?.to_owned();
            let req = templates::group_create(&waba, &a.name, a.description.as_deref(), &ids);
            app.send_and_emit(&req, field_id).await
        }
        GroupCommand::Get { id } => {
            validate::numeric_id(&id, "template group id")?;
            app.send_and_emit(&templates::group_get(&id), field_id)
                .await
        }
        GroupCommand::Update(a) => {
            validate::numeric_id(&a.id, "template group id")?;
            a.name.as_deref().map_or(Ok(()), name_ok)?;
            a.description.as_deref().map_or(Ok(()), description_ok)?;
            let add = integer_ids(&a.add)?;
            let remove = integer_ids(&a.remove)?;
            let update = GroupUpdate {
                name: a.name.as_deref(),
                description: a.description.as_deref(),
                add_templates: &add,
                remove_templates: &remove,
            };
            app.send_and_emit(&templates::group_update(&a.id, &update), |_| {
                Some(a.id.clone())
            })
            .await
        }
        GroupCommand::Delete { id } => {
            validate::numeric_id(&id, "template group id")?;
            if !app.confirm_or_abort(&format!("delete template group {id}"))? {
                return Ok(());
            }
            app.send_and_emit(&templates::group_delete(&id), |_| Some(id.clone()))
                .await
        }
    }
}
