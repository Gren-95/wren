#[derive(Debug)]
pub struct NavigationModel {
    back_stack: Vec<gio::File>,
    forward_stack: Vec<gio::File>,
    current: Option<gio::File>,
}

impl Default for NavigationModel {
    fn default() -> Self {
        Self {
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            current: None,
        }
    }
}

impl NavigationModel {
    pub fn navigate_to(&mut self, location: gio::File) -> Option<gio::File> {
        if let Some(current) = self.current.take() {
            self.back_stack.push(current);
        }
        self.forward_stack.clear();
        self.current = Some(location.clone());
        Some(location)
    }

    pub fn navigate_back(&mut self) -> Option<gio::File> {
        let prev = self.back_stack.pop()?;
        if let Some(current) = self.current.take() {
            self.forward_stack.push(current);
        }
        self.current = Some(prev.clone());
        Some(prev)
    }

    pub fn navigate_forward(&mut self) -> Option<gio::File> {
        let next = self.forward_stack.pop()?;
        if let Some(current) = self.current.take() {
            self.back_stack.push(current);
        }
        self.current = Some(next.clone());
        Some(next)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    pub fn current(&self) -> Option<&gio::File> {
        self.current.as_ref()
    }
}
