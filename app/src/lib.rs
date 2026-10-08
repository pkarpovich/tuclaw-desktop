pub mod agent_settings;
pub mod agents;
pub mod audio;
pub mod automation;
pub mod automations_panel;
pub mod automations_view;
pub mod badge;
pub mod card;
pub mod channels_view;
pub mod chrome;
pub mod composer;
pub mod control;
#[cfg(target_os = "macos")]
mod desktop;
pub mod failure;
pub mod feed;
pub mod form;
pub mod icon;
pub mod inspector;
pub mod link;
pub mod live;
pub mod local;
pub mod menu;
pub mod message;
pub mod notify;
pub mod people;
pub mod picture;
pub mod pictures;
pub mod plain;
pub mod profile_panel;
pub mod recorder;
pub mod rich;
pub mod runlog;
pub mod settings_panel;
pub mod shell;
pub mod sidebar;
pub mod state;
#[cfg(test)]
mod testing;
pub mod theme;
pub mod viewer;

#[cfg(target_os = "macos")]
pub use desktop::run;
