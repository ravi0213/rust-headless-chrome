use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;

use failure::Error;
use log::*;
use serde;

use crate::cdtp::target::methods::SetDiscoverTargets;
use crate::cdtp::{self, Event};
use crate::helpers::{wait_for, WaitOptions};
pub use crate::process::LaunchOptions;
use crate::process::Process;
use crate::tab::Tab;
use crate::transport::Transport;

/// Chrome browser.
pub struct Browser {
    _process: Process,
    transport: Arc<Transport>,
    tabs: Arc<Mutex<Vec<Arc<Tab>>>>,
}

impl Browser {
    /// Launch a new Chrome browser.
    ///
    /// The browser will have its data directory stored in a temporary directory.
    /// The browser proces wil be killed when this struct is dropeed.
    pub fn new(launch_options: LaunchOptions) -> Result<Self, Error> {
        let _process = Process::new(launch_options)?;

        let transport = Arc::new(Transport::new(_process.debug_ws_url.clone())?);

        let tabs = Arc::new(Mutex::new(vec![]));

        let browser = Browser {
            _process,
            tabs,
            transport,
        };

        let incoming_events_rx = browser.transport.listen_to_browser_events();
        browser.handle_incoming_messages(incoming_events_rx);

        // so we get events like 'targetCreated' and 'targetDestroyed'
        browser.call_method(SetDiscoverTargets { discover: true })?;

        Ok(browser)
    }

    pub fn get_tabs(&self) -> Arc<Mutex<Vec<Arc<Tab>>>> {
        Arc::clone(&self.tabs)
    }

    pub fn wait_for_initial_tab(&self) -> Result<Arc<Tab>, Error> {
        wait_for(
            || self.tabs.lock().unwrap().first().map(|tab| Arc::clone(tab)),
            WaitOptions {
                timeout_ms: 5000,
                sleep_ms: 10,
            },
        )
    }

    //    pub fn new_tab(&self) -> Result<Arc<Tab>, Error> {
    //        let create_target = target::methods::CreateTarget {
    //            url: "about:blank",
    //            width: None,
    //            height: None,
    //            browser_context_id: None,
    //            enable_begin_frame_control: None,
    //        };
    //
    ////        let target_id = self.call_method(create_target)?.target_id;
    ////        let new_tab = Arc::new(Tab::new(target_id, Arc::clone(&self.transport))?);
    ////
    ////        self.add_tab(Arc::clone(&new_tab));
    //
    //        Ok(new_tab)
    //    }

    fn handle_incoming_messages(&self, events_rx: mpsc::Receiver<Event>) {
        let tabs = Arc::clone(&self.tabs);
        let transport = Arc::clone(&self.transport);

        std::thread::spawn(move || {
            for event in events_rx {
                match event {
                    Event::TargetCreated(ev) => {
                        let target_info = ev.params.target_info;
                        trace!("Target created: {:?}", target_info);
                        if target_info.target_type.is_page() {
                            let new_tab =
                                Arc::new(Tab::new(target_info, Arc::clone(&transport)).unwrap());
                            tabs.lock().unwrap().push(new_tab);
                        }
                    }
                    Event::TargetInfoChanged(ev) => {
                        let target_info = ev.params.target_info;
                        trace!("Target info changed: {:?}", target_info);
                        if target_info.target_type.is_page() {
                            let locked_tabs = tabs.lock().unwrap();
                            let updated_tab = locked_tabs
                                .iter()
                                .find(|tab| *tab.get_target_id() == target_info.target_id)
                                .expect("got TargetInfoChanged event about a tab not in our list");
                            updated_tab.update_target_info(target_info);
                        }
                    }
                    Event::TargetDestroyed(_) => {}
                    _ => {}
                }
            }
        });
    }

    /// Call a browser method.
    ///
    /// See the `cdtp` module documentation for available methods.
    pub fn call_method<C>(&self, method: C) -> Result<C::ReturnObject, Error>
    where
        C: cdtp::Method + serde::Serialize,
    {
        self.transport.call_method(method)
    }

    #[cfg(test)]
    pub(crate) fn process(&self) -> &Process {
        &self._process
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time;

    #[test]
    fn browser_basic_test() {
        use crate::cdtp::target::methods::GetTargets;
        use crate::logging;

        fn try_out_browser() -> Result<(), Error> {
            let browser = Browser::new(LaunchOptions::default().unwrap().headless(true))?;

            let method = GetTargets {};
            let _targets = browser.call_method(method)?.target_infos;
            let tab = browser.wait_for_initial_tab()?;
            tab.navigate_to("https://wikipedia.org")?;
            std::thread::sleep(time::Duration::from_secs(4));
            Ok(())
        }
        logging::enable_logging();
        try_out_browser().expect("returned error");
    }

    #[test]
    fn ctrlc_chrome() {
        use crate::logging;
        logging::enable_logging();
        let _browser = Browser::new(LaunchOptions::default().unwrap().headless(false)).unwrap();
        std::thread::sleep(time::Duration::from_secs(40));
    }

    // things to test:
    // chrome comes with one target there by default.
}
