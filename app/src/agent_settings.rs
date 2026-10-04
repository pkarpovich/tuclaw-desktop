use tuclaw_core::model::AgentId;
use tuclaw_core::v3::{self, Role, SurfaceId, WiringChange};

use crate::link;
use crate::state::Inspector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub agent: AgentId,
    pub back: Option<Inspector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saving {
    Idle,
    Saving,
    Saved,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Description,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicRow {
    pub surface: SurfaceId,
    pub name: String,
    pub role: Role,
    pub listens: bool,
    pub home: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joinable {
    pub surface: SurfaceId,
    pub name: String,
    pub lead: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undo {
    Rewire {
        surface: SurfaceId,
        agent: AgentId,
        change: WiringChange,
    },
    Restore {
        surface: SurfaceId,
        agent: AgentId,
        change: WiringChange,
        demoted: Option<AgentId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub undo: Undo,
}

pub fn topics_of(
    surfaces: &[v3::Surface],
    directory: &[v3::Agent],
    agent: AgentId,
) -> Vec<TopicRow> {
    let wanted = link::v3_agent_id(agent);
    let mut home = None;
    for candidate in directory {
        if candidate.id == wanted {
            home = candidate.home_surface_id;
        }
    }
    let mut rows = Vec::new();
    for surface in surfaces {
        for wiring in &surface.agents {
            if wiring.agent_id != wanted {
                continue;
            }
            rows.push(TopicRow {
                surface: surface.id,
                name: surface.name.clone(),
                role: wiring.role,
                listens: wiring.listens,
                home: home == Some(surface.id),
            });
        }
    }
    rows
}

pub fn joinable(
    surfaces: &[v3::Surface],
    directory: &[v3::Agent],
    agent: AgentId,
) -> Vec<Joinable> {
    let wanted = link::v3_agent_id(agent);
    let mut options = Vec::new();
    for surface in surfaces {
        let mut wired = false;
        for wiring in &surface.agents {
            if wiring.agent_id == wanted {
                wired = true;
            }
        }
        if wired {
            continue;
        }
        let mut lead = None;
        for candidate in directory {
            if Some(candidate.id) == surface.lead_agent_id {
                lead = Some(candidate.name.clone());
            }
        }
        options.push(Joinable {
            surface: surface.id,
            name: surface.name.clone(),
            lead,
        });
    }
    options
}

pub fn joining_role(option: &Joinable) -> Role {
    match option.lead {
        Some(_) => Role::Mention,
        None => Role::Lead,
    }
}

#[cfg(test)]
mod tests {
    use tuclaw_core::model::AgentId;
    use tuclaw_core::v3::{self, Role, SurfaceId};

    use super::{joinable, joining_role, topics_of};

    fn world() -> (Vec<v3::Surface>, Vec<v3::Agent>) {
        let surfaces = serde_json::from_str(include_str!("../../core/testdata/v3/surfaces.json"))
            .expect("surfaces");
        let agents = serde_json::from_str(include_str!("../../core/testdata/v3/agents.json"))
            .expect("agents");
        (surfaces, agents)
    }

    #[test]
    fn an_agent_lists_the_topics_it_is_wired_to_with_its_home() {
        let (surfaces, agents) = world();
        let rows = topics_of(&surfaces, &agents, AgentId(1));
        assert!(!rows.is_empty());
        let mut home = 0;
        for row in &rows {
            if row.home {
                home += 1;
                assert_eq!(row.surface, SurfaceId(1));
            }
        }
        assert_eq!(home, 1);
        assert_eq!(rows[0].role, Role::Lead);
    }

    #[test]
    fn joinable_topics_skip_the_wired_ones_and_name_their_lead() {
        let (surfaces, agents) = world();
        let wired = topics_of(&surfaces, &agents, AgentId(1));
        let options = joinable(&surfaces, &agents, AgentId(1));
        assert_eq!(wired.len() + options.len(), surfaces.len());
        for option in &options {
            for row in &wired {
                assert_ne!(option.surface, row.surface);
            }
            match &option.lead {
                Some(_) => assert_eq!(joining_role(option), Role::Mention),
                None => assert_eq!(joining_role(option), Role::Lead),
            }
        }
    }
}
