impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_event_mirror(&mut self, mirror: Option<Arc<Mutex<Vec<Event>>>>) {
        self.event_mirror = mirror;
    }

    pub fn turn_interrupt_registry(&self) -> TurnInterruptRegistry {
        self.turn_interrupts.clone()
    }

    pub fn set_turn_interrupt_registry(&mut self, registry: TurnInterruptRegistry) {
        self.turn_interrupts = registry;
    }

    pub fn set_memory_backend_runner(&mut self, runner: Box<dyn MemoryBackendRunner>) {
        self.memory_backend_runner = Some(Arc::from(runner));
    }

    #[cfg(test)]
    pub fn set_memory_backend_runner_for_test(&mut self, runner: Box<dyn MemoryBackendRunner>) {
        self.set_memory_backend_runner(runner);
    }

    pub fn initialize(&self, params: InitializeParams) -> InitializeResult {
        let server_capabilities = server_capabilities();
        InitializeResult {
            protocol_version: OPPI_PROTOCOL_VERSION.to_string(),
            min_protocol_version: OPPI_MIN_PROTOCOL_VERSION.to_string(),
            protocol_compatible: params
                .protocol_version
                .as_deref()
                .is_none_or(protocol_version_is_compatible),
            server_name: "oppi-server".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            accepted_client_capabilities: params
                .client_capabilities
                .into_iter()
                .filter(|capability| server_capabilities.contains(capability))
                .collect(),
            server_capabilities,
        }
    }

    pub fn prepare_command(
        &self,
        params: CommandPrepareParams,
    ) -> Result<CommandPrepareResult, RuntimeError> {
        match prepare_runtime_command(params.clone()) {
            Ok(result) => Ok(result),
            Err(error) if error.code == "command_prepare_unknown" => {
                match self.expand_prompt_template(PromptTemplateExpandParams {
                    thread_id: params.thread_id.clone(),
                    name: params.command.clone(),
                    args: params.args.clone(),
                    prompt_variant_append: params.prompt_variant_append.clone(),
                }) {
                    Ok(expanded) => Ok(CommandPrepareResult {
                        command: expanded.name,
                        input: expanded.input,
                        system_prompt_profile: None,
                        notes: expanded.notes,
                    }),
                    Err(template_error) if template_error.code == "prompt_template_not_found" => {
                        Err(error)
                    }
                    Err(template_error) => Err(template_error),
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn list_threads(&self) -> RuntimeListResult<Thread> {
        RuntimeListResult {
            items: self.threads.values().cloned().collect(),
        }
    }

    pub fn list_thread_items(
        &self,
        params: ThreadItemsParams,
    ) -> Result<ThreadItemsResult, RuntimeError> {
        let branch = self.thread_item_branch(&params.thread_id)?;
        let filter = params.filter.unwrap_or_default();
        let limit = params
            .limit
            .unwrap_or(DEFAULT_EVENTS_LIST_LIMIT)
            .min(MAX_EVENTS_LIST_LIMIT);
        let mut parent_event_id = None;
        let mut entries: Vec<ThreadItemEntry> = Vec::new();
        for event in self
            .event_store
            .list_thread(&params.thread_id)
            .map_err(event_store_error)?
        {
            let EventKind::ItemCompleted { item } = event.kind else {
                continue;
            };
            let labels = self
                .thread_item_labels
                .get(&(params.thread_id.clone(), event.id))
                .cloned()
                .unwrap_or_default();
            if !thread_item_matches_filter(&item.kind, &labels, filter) {
                continue;
            }
            let entry = ThreadItemEntry {
                event_id: event.id,
                parent_event_id,
                child_event_ids: Vec::new(),
                completed_at_ms: event.created_at_ms,
                labels,
                item,
            };
            if let Some(previous_entry) = entries.last_mut() {
                previous_entry.child_event_ids.push(event.id);
            }
            parent_event_id = Some(event.id);
            if entry.event_id > params.after {
                entries.push(entry);
            }
        }
        let total_count = entries.len() as u64;
        entries.truncate(limit);
        let items = entries.iter().map(|entry| entry.item.clone()).collect();
        Ok(ThreadItemsResult {
            thread_id: params.thread_id,
            branch,
            filter,
            after: params.after,
            entries,
            items,
            total_count,
        })
    }

    fn thread_item_branch(&self, thread_id: &str) -> Result<ThreadItemBranch, RuntimeError> {
        let thread = self.thread(thread_id)?;
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(thread.id.clone());
        while let Some(current_id) = current {
            if !seen.insert(current_id.clone()) {
                break;
            }
            path.push(current_id.clone());
            current = self
                .threads
                .get(&current_id)
                .and_then(|thread| thread.forked_from.clone());
        }
        path.reverse();
        let depth = path.len().saturating_sub(1) as u64;
        let parent_thread_id = thread.forked_from.clone();
        let fork_event_id = self.thread_fork_points.get(thread_id).copied();
        let child_thread_ids = self.thread_child_ids(thread_id);
        let sibling_thread_ids = parent_thread_id
            .as_deref()
            .map(|parent| {
                self.thread_child_ids(parent)
                    .into_iter()
                    .filter(|child_id| child_id.as_str() != thread_id)
                    .collect()
            })
            .unwrap_or_default();
        Ok(ThreadItemBranch {
            thread_id: thread.id.clone(),
            parent_thread_id,
            fork_event_id,
            child_thread_ids,
            sibling_thread_ids,
            path,
            depth,
        })
    }

    fn thread_child_ids(&self, thread_id: &str) -> Vec<ThreadId> {
        self.threads
            .values()
            .filter(|thread| thread.forked_from.as_deref() == Some(thread_id))
            .map(|thread| thread.id.clone())
            .collect()
    }

    pub fn set_thread_item_labels(
        &mut self,
        params: ThreadItemLabelsUpdateParams,
    ) -> Result<ThreadItemLabelsResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.ensure_thread_item_event(&params.thread_id, params.event_id)?;
        let labels = normalize_thread_item_labels(params.labels);
        let key = (params.thread_id.clone(), params.event_id);
        if labels.is_empty() {
            self.thread_item_labels.remove(&key);
        } else {
            self.thread_item_labels.insert(key, labels.clone());
        }
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::ThreadItemLabelsUpdated {
                target_event_id: params.event_id,
                labels: labels.clone(),
            },
        );
        Ok(ThreadItemLabelsResult {
            thread_id: params.thread_id,
            event_id: params.event_id,
            labels,
            events: vec![event],
        })
    }

    fn ensure_thread_item_event(
        &self,
        thread_id: &str,
        event_id: EventId,
    ) -> Result<(), RuntimeError> {
        let exists = self
            .event_store
            .list_thread(thread_id)
            .map_err(event_store_error)?
            .into_iter()
            .any(|event| {
                event.id == event_id && matches!(event.kind, EventKind::ItemCompleted { .. })
            });
        if exists {
            Ok(())
        } else {
            Err(not_found("thread item event", &event_id.to_string()))
        }
    }

    fn user_thread_item_history_cutoff(
        &self,
        thread_id: &str,
        event_id: EventId,
    ) -> Result<EventId, RuntimeError> {
        let mut cutoff_event_id = 0;
        for event in self
            .event_store
            .list_thread(thread_id)
            .map_err(event_store_error)?
        {
            if event.id == event_id {
                return match event.kind {
                    EventKind::ItemCompleted {
                        item:
                            Item {
                                kind: ItemKind::UserMessage { .. },
                                ..
                            },
                    } => Ok(cutoff_event_id),
                    EventKind::ItemCompleted { .. } => Err(RuntimeError::new(
                        "thread_item_not_user_message",
                        RuntimeErrorCategory::InvalidRequest,
                        format!("thread item event {event_id} is not a user message"),
                    )),
                    _ => Err(not_found("thread item event", &event_id.to_string())),
                };
            }
            cutoff_event_id = event.id;
        }
        Err(not_found("thread item event", &event_id.to_string()))
    }

    pub fn start_thread(&mut self, params: ThreadStartParams) -> ThreadStartResult {
        self.next_thread += 1;
        let thread = Thread {
            id: format!("thread-{}", self.next_thread),
            project: params.project,
            status: ThreadStatus::Active,
            title: params.title,
            forked_from: None,
        };
        self.threads.insert(thread.id.clone(), thread.clone());
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadStarted {
                thread: thread.clone(),
            },
        );
        ThreadStartResult {
            thread,
            events: vec![event],
        }
    }

    pub fn resume_thread(&mut self, thread_id: &str) -> Result<ThreadStartResult, RuntimeError> {
        let thread = self.thread(thread_id)?.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadResumed {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn fork_thread(
        &mut self,
        from_thread_id: &str,
        title: Option<String>,
    ) -> Result<ThreadStartResult, RuntimeError> {
        let source = self.thread(from_thread_id)?.clone();
        self.next_thread += 1;
        let thread = Thread {
            id: format!("thread-{}", self.next_thread),
            project: source.project,
            status: ThreadStatus::Active,
            title,
            forked_from: Some(source.id.clone()),
        };
        self.threads.insert(thread.id.clone(), thread.clone());
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadForked {
                thread: thread.clone(),
                from_thread_id: source.id,
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn fork_thread_from_item(
        &mut self,
        params: ThreadItemForkParams,
    ) -> Result<ThreadStartResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.ensure_thread_item_event(&params.thread_id, params.event_id)?;
        let source_thread_id = params.thread_id;
        let source_event_id = params.event_id;
        let mut result = self.fork_thread(&source_thread_id, params.title)?;
        let child_thread_id = result.thread.id.clone();
        self.thread_fork_points
            .insert(child_thread_id.clone(), source_event_id);
        let event = self.event(
            &child_thread_id,
            None,
            EventKind::ThreadForkPointSet {
                thread_id: child_thread_id.clone(),
                source_thread_id,
                source_event_id,
                history_cutoff_event_id: None,
            },
        );
        result.events.push(event);
        Ok(result)
    }

    pub fn fork_thread_from_item_edit(
        &mut self,
        params: ThreadItemEditForkParams,
    ) -> Result<ThreadItemEditForkResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let input = params.input.trim().to_string();
        if input.is_empty() {
            return Err(RuntimeError::new(
                "thread_item_edit_input_empty",
                RuntimeErrorCategory::InvalidRequest,
                "thread item edit input must not be empty",
            ));
        }
        let source_thread_id = params.thread_id;
        let source_event_id = params.event_id;
        let history_cutoff_event_id =
            self.user_thread_item_history_cutoff(&source_thread_id, source_event_id)?;
        let mut result = self.fork_thread(&source_thread_id, params.title)?;
        let child_thread_id = result.thread.id.clone();
        self.thread_fork_points
            .insert(child_thread_id.clone(), source_event_id);
        self.thread_history_cutoffs
            .insert(child_thread_id.clone(), history_cutoff_event_id);
        let event = self.event(
            &child_thread_id,
            None,
            EventKind::ThreadForkPointSet {
                thread_id: child_thread_id.clone(),
                source_thread_id,
                source_event_id,
                history_cutoff_event_id: Some(history_cutoff_event_id),
            },
        );
        result.events.push(event);
        Ok(ThreadItemEditForkResult {
            thread: result.thread,
            source_event_id,
            history_cutoff_event_id,
            input,
            events: result.events,
        })
    }

    pub fn rename_thread(
        &mut self,
        thread_id: &str,
        title: String,
    ) -> Result<ThreadStartResult, RuntimeError> {
        let title = normalize_thread_title(&title)?;
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))?;
        thread.title = title;
        let thread = thread.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadUpdated {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn archive_thread(&mut self, thread_id: &str) -> Result<ThreadStartResult, RuntimeError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))?;
        thread.status = ThreadStatus::Archived;
        let thread = thread.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadUpdated {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn get_thread_goal(
        &self,
        params: ThreadGoalGetParams,
    ) -> Result<ThreadGoalGetResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        Ok(ThreadGoalGetResult {
            goal: self.goals.get(&params.thread_id).cloned(),
        })
    }

    pub fn set_thread_goal(
        &mut self,
        params: ThreadGoalSetParams,
    ) -> Result<ThreadGoalSetResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let thread_id = params.thread_id;
        let now_ms = now_millis();
        let mut goal = if let Some(objective) = params.objective {
            let objective = validate_goal_objective(&objective).map_err(invalid_goal_request)?;
            let token_budget = params.token_budget.unwrap_or(None);
            validate_goal_budget(token_budget).map_err(invalid_goal_request)?;
            self.goal_continuations.remove(&thread_id);
            new_goal(
                thread_id.clone(),
                objective,
                params.status.unwrap_or(ThreadGoalStatus::Active),
                token_budget,
                now_ms,
            )
        } else {
            let mut goal = self
                .goals
                .get(&thread_id)
                .cloned()
                .ok_or_else(|| not_found("thread goal", &thread_id))?;
            if let Some(status) = params.status {
                goal.status = status;
            }
            if let Some(token_budget) = params.token_budget {
                validate_goal_budget(token_budget).map_err(invalid_goal_request)?;
                goal.token_budget = token_budget;
            }
            goal.updated_at_ms = now_ms;
            goal
        };
        goal.status = status_after_budget(&goal);
        if goal.status == ThreadGoalStatus::Active {
            self.goal_active_started_at_ms
                .entry(thread_id.clone())
                .or_insert(now_ms);
        } else {
            self.goal_active_started_at_ms.remove(&thread_id);
        }
        self.goals.insert(thread_id.clone(), goal.clone());
        self.event(
            &thread_id,
            None,
            EventKind::ThreadGoalUpdated { goal: goal.clone() },
        );
        Ok(ThreadGoalSetResult { goal })
    }

    pub fn clear_thread_goal(
        &mut self,
        params: ThreadGoalClearParams,
    ) -> Result<ThreadGoalClearResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let cleared = self.goals.remove(&params.thread_id).is_some();
        self.goal_active_started_at_ms.remove(&params.thread_id);
        self.goal_continuations.remove(&params.thread_id);
        if cleared {
            self.event(
                &params.thread_id,
                None,
                EventKind::ThreadGoalCleared {
                    thread_id: params.thread_id.clone(),
                },
            );
        }
        Ok(ThreadGoalClearResult { cleared })
    }

    pub fn render_thread_goal_continuation_prompt(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        self.thread(thread_id)?;
        Ok(self.goals.get(thread_id).and_then(|goal| {
            (goal.status == ThreadGoalStatus::Active).then(|| render_goal_continuation_prompt(goal))
        }))
    }

    pub fn render_thread_goal_budget_limit_prompt(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        self.thread(thread_id)?;
        Ok(self.goals.get(thread_id).and_then(|goal| {
            (goal.status == ThreadGoalStatus::BudgetLimited)
                .then(|| render_goal_budget_limit_prompt(goal))
        }))
    }

    pub fn next_thread_goal_continuation(
        &mut self,
        params: ThreadGoalContinuationParams,
    ) -> Result<ThreadGoalContinuationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let Some(goal) = self.goals.get(&params.thread_id).cloned() else {
            return Ok(ThreadGoalContinuationResult {
                goal: None,
                prompt: None,
                continuation: None,
                blocked_reason: Some("no active thread goal".to_string()),
            });
        };
        if goal.status != ThreadGoalStatus::Active {
            return Ok(ThreadGoalContinuationResult {
                goal: Some(goal.clone()),
                prompt: None,
                continuation: None,
                blocked_reason: Some(format!(
                    "thread goal is {}; automatic continuation only runs for active goals",
                    thread_goal_status_name(goal.status)
                )),
            });
        }

        let max_continuations = params.max_continuations.unwrap_or(MAX_CONTINUATIONS);
        let continuation = self
            .goal_continuations
            .entry(params.thread_id.clone())
            .and_modify(|value| *value = value.saturating_add(1))
            .or_insert(1);
        let continuation = *continuation;
        if continuation > max_continuations {
            let mut blocked_goal = goal.clone();
            blocked_goal.status = ThreadGoalStatus::Paused;
            blocked_goal.updated_at_ms = now_millis();
            self.goals
                .insert(params.thread_id.clone(), blocked_goal.clone());
            self.goal_active_started_at_ms.remove(&params.thread_id);
            self.event(
                &params.thread_id,
                None,
                EventKind::ThreadGoalUpdated {
                    goal: blocked_goal.clone(),
                },
            );
            return Ok(ThreadGoalContinuationResult {
                goal: Some(blocked_goal),
                prompt: None,
                continuation: Some(continuation),
                blocked_reason: Some(format!(
                    "goal continuation guard reached after {max_continuations} continuation(s)"
                )),
            });
        }

        Ok(ThreadGoalContinuationResult {
            goal: Some(goal.clone()),
            prompt: Some(render_goal_continuation_prompt(&goal)),
            continuation: Some(continuation),
            blocked_reason: None,
        })
    }

    fn begin_goal_turn_accounting(&mut self, thread_id: &str) {
        if self
            .goals
            .get(thread_id)
            .is_some_and(|goal| goal.status == ThreadGoalStatus::Active)
        {
            self.goal_active_started_at_ms
                .entry(thread_id.to_string())
                .or_insert_with(now_millis);
        }
    }

    fn account_active_goal(
        &mut self,
        thread_id: &str,
        token_delta: i64,
        elapsed_seconds: i64,
        emitted: &mut Vec<Event>,
    ) {
        if token_delta <= 0 && elapsed_seconds <= 0 {
            return;
        }
        let now_ms = now_millis();
        let updated = self.goals.get_mut(thread_id).and_then(|goal| {
            if matches!(
                goal.status,
                ThreadGoalStatus::Active | ThreadGoalStatus::BudgetLimited
            ) {
                apply_goal_accounting_delta(goal, token_delta, elapsed_seconds, now_ms);
                Some(goal.clone())
            } else {
                None
            }
        });
        if let Some(goal) = updated {
            emitted.push(self.event(thread_id, None, EventKind::ThreadGoalUpdated { goal }));
        }
    }

    fn finish_goal_turn_accounting(
        &mut self,
        thread_id: &str,
        pause_active: bool,
        emitted: &mut Vec<Event>,
    ) {
        let elapsed_seconds = self
            .goal_active_started_at_ms
            .remove(thread_id)
            .map(|started_ms| {
                let elapsed_ms = now_millis().saturating_sub(started_ms);
                (elapsed_ms / 1_000).min(i64::MAX as u64) as i64
            })
            .unwrap_or(0);
        self.account_active_goal(thread_id, 0, elapsed_seconds, emitted);
        if pause_active {
            let updated = self.goals.get_mut(thread_id).and_then(|goal| {
                if goal.status == ThreadGoalStatus::Active {
                    goal.status = ThreadGoalStatus::Paused;
                    goal.updated_at_ms = now_millis();
                    Some(goal.clone())
                } else {
                    None
                }
            });
            if let Some(goal) = updated {
                emitted.push(self.event(thread_id, None, EventKind::ThreadGoalUpdated { goal }));
            }
        }
    }

    pub fn start_turn(&mut self, params: TurnStartParams) -> Result<TurnStartResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let next_turn_id = format!("turn-{}", self.next_turn + 1);
        if let Some(simulated) = params.simulated_tool.as_ref() {
            self.validate_tool_batch_items(
                &params.thread_id,
                &next_turn_id,
                std::slice::from_ref(simulated),
            )?;
        }
        self.next_turn += 1;
        let mut turn = Turn {
            id: next_turn_id,
            thread_id: params.thread_id.clone(),
            status: TurnStatus::Running,
            phase: TurnPhase::Input,
            parent_turn_id: None,
        };
        self.turns.insert(turn.id.clone(), turn.clone());
        self.begin_goal_turn_accounting(&turn.thread_id);

        let mut emitted = Vec::new();
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnStarted { turn: turn.clone() },
        ));
        let user_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::UserMessage {
                text: params.input.clone(),
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: user_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item: user_item },
        ));

        for phase in [
            TurnPhase::Message,
            TurnPhase::History,
            TurnPhase::System,
            TurnPhase::Api,
            TurnPhase::Tokens,
        ] {
            self.set_phase(&mut turn, phase, &mut emitted);
        }

        if params.requested_continuations > MAX_CONTINUATIONS {
            self.set_phase(&mut turn, TurnPhase::Loop, &mut emitted);
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Error,
                message: format!(
                    "continuation guard blocked {} requested continuations; max is {MAX_CONTINUATIONS}",
                    params.requested_continuations
                ),
                metadata: BTreeMap::new(),
            };
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
            turn.status = TurnStatus::Aborted;
            self.turns.insert(turn.id.clone(), turn.clone());
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::TurnAborted {
                    reason: "continuation guard exceeded".to_string(),
                },
            ));
            return Ok(TurnStartResult {
                turn,
                events: emitted,
            });
        }

        self.set_phase(&mut turn, TurnPhase::Tools, &mut emitted);
        if let Some(simulated) = params.simulated_tool {
            emitted.extend(self.record_tool_pair(&turn.thread_id, &turn.id, simulated)?);
        }

        if params.defer_completion {
            self.turns.insert(turn.id.clone(), turn.clone());
            return Ok(TurnStartResult {
                turn,
                events: emitted,
            });
        }

        self.set_phase(&mut turn, TurnPhase::Loop, &mut emitted);
        self.set_phase(&mut turn, TurnPhase::Render, &mut emitted);
        let assistant_text = params.assistant_response.unwrap_or_else(|| {
            format!(
                "OPPi runtime accepted turn {} and represented it as semantic events.",
                turn.id
            )
        });
        let assistant_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::AssistantMessage {
                text: assistant_text,
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: assistant_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted {
                item: assistant_item,
            },
        ));

        self.set_phase(&mut turn, TurnPhase::Hooks, &mut emitted);
        if let Some(feedback) = params.stop_hook_feedback {
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Info,
                message: "stop-hook continuation recorded".to_string(),
                metadata: BTreeMap::from([("feedback".to_string(), feedback)]),
            };
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
        }
        self.set_phase(&mut turn, TurnPhase::Await, &mut emitted);

        turn.status = TurnStatus::Completed;
        self.turns.insert(turn.id.clone(), turn.clone());
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnCompleted {
                turn_id: turn.id.clone(),
            },
        ));

        Ok(TurnStartResult {
            turn,
            events: emitted,
        })
    }

    pub fn run_agentic_turn(
        &mut self,
        params: AgenticTurnParams,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.run_agentic_turn_with_parent(params, None, None)
    }

    fn run_agentic_turn_with_parent(
        &mut self,
        params: AgenticTurnParams,
        parent_turn_id: Option<TurnId>,
        restricted_tool_registry: Option<ToolRegistry>,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let input = params.input;
        let follow_up = params.follow_up;
        let sandbox_policy = params.sandbox_policy;
        self.next_turn += 1;
        let mut turn = Turn {
            id: format!("turn-{}", self.next_turn),
            thread_id: params.thread_id.clone(),
            status: TurnStatus::Running,
            phase: TurnPhase::Input,
            parent_turn_id,
        };
        self.turns.insert(turn.id.clone(), turn.clone());

        let mut emitted = Vec::new();
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnStarted { turn: turn.clone() },
        ));
        let user_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::UserMessage {
                text: input.clone(),
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: user_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item: user_item },
        ));

        let mut state = AgenticLoopState::new();
        for phase in [TurnPhase::Message, TurnPhase::History, TurnPhase::System] {
            self.transition_agentic_phase(&mut state, &mut turn, phase, &mut emitted)?;
        }
        if let Some(policy) = sandbox_policy {
            let mode = policy.permission_profile.mode;
            self.turn_sandbox_policies.insert(turn.id.clone(), policy);
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "sandbox policy attached to turn".to_string(),
                        metadata: BTreeMap::from([
                            ("component".to_string(), "permissions".to_string()),
                            ("mode".to_string(), mode.as_str().to_string()),
                        ]),
                    },
                },
            ));
        }
        if let Some(context) = follow_up.as_ref() {
            emitted.push(
                self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "follow-up chain context applied".to_string(),
                            metadata: BTreeMap::from([
                                ("component".to_string(), "follow-up".to_string()),
                                (
                                    "chainId".to_string(),
                                    context
                                        .chain_id
                                        .clone()
                                        .unwrap_or_else(|| "unknown".to_string()),
                                ),
                                (
                                    "pendingFollowUps".to_string(),
                                    follow_up_pending_count(context).to_string(),
                                ),
                            ]),
                        },
                    },
                ),
            );
        }
        let approved = params.approved_tool_call_ids.into_iter().collect();
        if restricted_tool_registry.is_none()
            && let Err(error) = self.plugin_runner_tool_definitions_for_thread(&turn.thread_id)
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Warning,
                        message: error.message,
                        metadata: BTreeMap::from([
                            ("component".to_string(), "plugin-runner".to_string()),
                            ("code".to_string(), error.code),
                        ]),
                    },
                },
            ));
        }
        let tool_registry = restricted_tool_registry
            .unwrap_or_else(|| self.turn_tool_registry(params.tool_definitions));
        let mut model_provider_config =
            append_follow_up_to_model_provider(params.model_provider, follow_up.as_ref(), &input);
        if model_provider_config.is_some()
            && let Some(context) = self.mcp_subscribed_resource_context(&turn.thread_id)?
            && let Some(config) = model_provider_config.as_mut()
        {
            config.system_prompt = Some(append_system_prompt(
                config.system_prompt.take(),
                "Subscribed MCP resource context",
                &context.prompt,
            ));
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "MCP subscribed resources injected".to_string(),
                        metadata: BTreeMap::from([
                            ("component".to_string(), "mcp".to_string()),
                            ("resources".to_string(), context.resources.to_string()),
                            ("skipped".to_string(), context.skipped.to_string()),
                        ]),
                    },
                },
            ));
        }
        if let Some(config) = model_provider_config.as_mut()
            && let Some((skill_prompt, skills)) =
                self.skill_injection_prompt(&turn.thread_id, &input, &[])?
        {
            config.system_prompt = Some(append_system_prompt(
                config.system_prompt.take(),
                "Relevant OPPi skill instructions",
                &skill_prompt,
            ));
            emitted.push(
                self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "skill instructions injected".to_string(),
                            metadata: BTreeMap::from([(
                                "skills".to_string(),
                                skills
                                    .iter()
                                    .map(|skill| skill.name.clone())
                                    .collect::<Vec<_>>()
                                    .join(","),
                            )]),
                        },
                    },
                ),
            );
        }
        let mut provider = model_provider(
            model_provider_config,
            params.model_steps,
            model_visible_tool_definitions(&tool_registry),
            None,
        )?;
        self.drive_agentic_turn(
            turn,
            state,
            input,
            provider.as_mut(),
            &tool_registry,
            approved,
            params.cancellation,
            params.max_continuations.unwrap_or(MAX_CONTINUATIONS),
            0,
            Vec::new(),
            emitted,
        )
    }

    pub fn resume_agentic_turn(
        &mut self,
        params: AgenticTurnResumeParams,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.resume_agentic_turn_with_registry(params, None)
    }

    fn resume_agentic_turn_with_registry(
        &mut self,
        params: AgenticTurnResumeParams,
        restricted_tool_registry: Option<ToolRegistry>,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let mut turn = self
            .turn_in_thread(&params.thread_id, &params.turn_id)?
            .clone();
        self.begin_goal_turn_accounting(&turn.thread_id);
        let ask_user_response = params.ask_user_response;
        if ask_user_response.is_some() {
            if turn.status != TurnStatus::WaitingForUser {
                return Err(RuntimeError::new(
                    "turn_not_waiting_for_user",
                    RuntimeErrorCategory::TerminalState,
                    format!("turn {} is not waiting for user input", turn.id),
                ));
            }
        } else if turn.status != TurnStatus::WaitingForApproval {
            return Err(RuntimeError::new(
                "turn_not_waiting_for_approval",
                RuntimeErrorCategory::TerminalState,
                format!("turn {} is not waiting for approval", turn.id),
            ));
        }
        let approved: BTreeSet<_> = params.approved_tool_call_ids.into_iter().collect();
        let mut emitted = Vec::new();
        let mut answered_tool_result = None;
        if let Some(response) = ask_user_response {
            let tool_call = {
                let owner = self.question_in_thread_mut(&params.thread_id, &response.request_id)?;
                if owner.turn_id.as_deref() != Some(&params.turn_id) {
                    return Err(RuntimeError::new(
                        "question_turn_mismatch",
                        RuntimeErrorCategory::InvalidRequest,
                        format!(
                            "question {} does not belong to turn {}",
                            response.request_id, params.turn_id
                        ),
                    ));
                }
                if owner.resolved {
                    return Err(already_resolved("question", &response.request_id));
                }
                owner.resolved = true;
                let linked_tool_call_id = owner.tool_call_id.clone();
                owner.tool_call.clone().ok_or_else(|| {
                    RuntimeError::new(
                        "question_missing_tool_call",
                        RuntimeErrorCategory::InvalidRequest,
                        format!(
                            "question {} is not linked to a resumable tool call{}",
                            response.request_id,
                            linked_tool_call_id
                                .as_ref()
                                .map(|id| format!(" (toolCallId: {id})"))
                                .unwrap_or_default()
                        ),
                    )
                })?
            };
            let output = ask_user_response_text(&response);
            let result = ToolResult {
                call_id: tool_call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            };
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::AskUserResolved { response },
            ));
            answered_tool_result = Some((tool_call, result));
        }
        let mut approval_events = Vec::new();
        for (approval_id, approval) in self.approvals.iter_mut() {
            if approval.thread_id == params.thread_id
                && approval.turn_id.as_deref() == Some(&params.turn_id)
                && approval.outcome.is_none()
                && approval
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|call_id| approved.contains(call_id))
            {
                approval.outcome = Some(ApprovalOutcome::Approved);
                approval_events.push(ApprovalDecision {
                    request_id: approval_id.clone(),
                    decision: ApprovalOutcome::Approved,
                    message: Some("approved for agentic turn resume".to_string()),
                });
            }
        }
        for decision in approval_events {
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ApprovalResolved { decision },
            ));
        }
        turn.status = TurnStatus::Running;
        let state = AgenticLoopState {
            current: TurnPhase::Loop,
        };
        if let Some(policy) = params.sandbox_policy {
            self.turn_sandbox_policies.insert(turn.id.clone(), policy);
        }
        if restricted_tool_registry.is_none()
            && let Err(error) = self.plugin_runner_tool_definitions_for_thread(&turn.thread_id)
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Warning,
                        message: error.message,
                        metadata: BTreeMap::from([
                            ("component".to_string(), "plugin-runner".to_string()),
                            ("code".to_string(), error.code),
                        ]),
                    },
                },
            ));
        }
        let tool_registry = restricted_tool_registry
            .unwrap_or_else(|| self.turn_tool_registry(params.tool_definitions));
        let resume_state = self.direct_provider_turns.remove(&params.turn_id);
        let pending_tool_calls = if resume_state.is_some() {
            self.pending_approved_tool_calls(&params.thread_id, &params.turn_id, &approved)
        } else {
            Vec::new()
        };
        let follow_up = if resume_state.is_none() {
            params.follow_up.as_ref()
        } else {
            None
        };
        let model_provider_config = append_follow_up_to_model_provider(
            params.model_provider,
            follow_up,
            "resume approved agentic turn",
        );
        let mut provider = model_provider(
            model_provider_config,
            params.model_steps,
            model_visible_tool_definitions(&tool_registry),
            resume_state,
        )?;
        if let Some((tool_call, result)) = answered_tool_result {
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolCallCompleted {
                    result: result.clone(),
                },
            ));
            provider.observe_tool_result(&tool_call, &result)?;
        }
        self.drive_agentic_turn(
            turn,
            state,
            "resume approved agentic turn".to_string(),
            provider.as_mut(),
            &tool_registry,
            approved,
            params.cancellation,
            params.max_continuations.unwrap_or(MAX_CONTINUATIONS),
            0,
            pending_tool_calls,
            emitted,
        )
    }

    pub fn record_tool(
        &mut self,
        params: ToolRecordParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn = self.turn_in_thread(&params.thread_id, &params.turn_id)?;
        ensure_turn_mutable(turn)?;
        let simulated = SimulatedToolUse {
            call: params.call,
            result: params.result,
            require_approval: false,
            concurrency_safe: false,
        };
        let events = self.record_tool_pair(&params.thread_id, &params.turn_id, simulated)?;
        Ok(EventsListResult { events })
    }

    pub fn record_tool_batch(
        &mut self,
        params: ToolBatchRecordParams,
    ) -> Result<ToolBatchRecordResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn = self.turn_in_thread(&params.thread_id, &params.turn_id)?;
        ensure_turn_mutable(turn)?;
        if params.tools.is_empty() {
            return Ok(ToolBatchRecordResult {
                batches: Vec::new(),
                events: Vec::new(),
            });
        }

        let planned = partition_ordered_tool_batches(params.tools);
        for batch in &planned {
            self.validate_tool_batch_items(&params.thread_id, &params.turn_id, &batch.tools)?;
        }
        let mut batches = Vec::new();
        let mut events = Vec::new();
        for (index, batch) in planned.into_iter().enumerate() {
            let batch_id = format!("{}-batch-{}", params.turn_id, index + 1);
            let descriptor = describe_tool_batch(batch_id.clone(), &batch, params.max_concurrency);
            events.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolBatchStarted {
                    batch: descriptor.clone(),
                },
            ));
            events.extend(self.record_tool_batch_items(
                &params.thread_id,
                &params.turn_id,
                batch.tools,
                batch.execution,
            )?);
            let status = if descriptor.tool_call_ids.is_empty() {
                ToolBatchStatus::Aborted
            } else {
                ToolBatchStatus::Completed
            };
            events.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolBatchCompleted {
                    batch_id: batch_id.clone(),
                    status,
                },
            ));
            batches.push(descriptor);
        }
        Ok(ToolBatchRecordResult { batches, events })
    }

    pub fn steer_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        input: String,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let turn = self.turn_in_thread(thread_id, turn_id)?.clone();
        ensure_turn_mutable(&turn)?;
        let item = self.item(thread_id, &turn.id, ItemKind::UserMessage { text: input });
        let event = self.event(thread_id, Some(&turn.id), EventKind::ItemCompleted { item });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn interrupt_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        reason: String,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let mut turn = self.turn_in_thread(thread_id, turn_id)?.clone();
        ensure_turn_mutable(&turn)?;
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        let _ = self.turn_interrupts.take(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        let event = self.event(
            thread_id,
            Some(&turn.id),
            EventKind::TurnInterrupted { reason },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn request_approval(
        &mut self,
        thread_id: &str,
        mut request: ApprovalRequest,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        if request.id.is_empty() {
            self.next_approval += 1;
            request.id = format!("approval-{}", self.next_approval);
        }
        if self.approvals.contains_key(&request.id) {
            return Err(already_exists("approval", &request.id));
        }
        self.approvals.insert(
            request.id.clone(),
            OwnedApprovalRequest {
                thread_id: thread_id.to_string(),
                turn_id: None,
                tool_call_id: request.tool_call.as_ref().map(|call| call.id.clone()),
                tool_call: request.tool_call.clone(),
                outcome: None,
            },
        );
        let event = self.event(thread_id, None, EventKind::ApprovalRequested { request });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn respond_approval(
        &mut self,
        params: ApprovalRespondParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn_id = {
            let owner = self.approval_in_thread_mut(&params.thread_id, &params.request_id)?;
            if owner.outcome.is_some() {
                return Err(already_resolved("approval", &params.request_id));
            }
            owner.outcome = Some(params.decision);
            owner.turn_id.clone()
        };
        let decision = ApprovalDecision {
            request_id: params.request_id,
            decision: params.decision,
            message: params.message,
        };
        let event = self.event(
            &params.thread_id,
            turn_id.as_deref(),
            EventKind::ApprovalResolved { decision },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn request_question(
        &mut self,
        thread_id: &str,
        mut request: QuestionRequest,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        if request.id.is_empty() {
            self.next_question += 1;
            request.id = format!("question-{}", self.next_question);
        }
        if self.questions.contains_key(&request.id) {
            return Err(already_exists("question", &request.id));
        }
        self.questions.insert(
            request.id.clone(),
            OwnedQuestionRequest {
                thread_id: thread_id.to_string(),
                turn_id: None,
                tool_call_id: None,
                tool_call: None,
                resolved: false,
            },
        );
        let event = self.event(thread_id, None, EventKind::QuestionRequested { request });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn respond_question(
        &mut self,
        params: QuestionRespondParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        {
            let owner = self.question_in_thread_mut(&params.thread_id, &params.request_id)?;
            if owner.resolved {
                return Err(already_resolved("question", &params.request_id));
            }
            owner.resolved = true;
        }
        let response = QuestionResponse {
            request_id: params.request_id,
            answer: params.answer,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::QuestionResolved { response },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_plugin(
        &mut self,
        thread_id: &str,
        plugin: PluginRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        self.plugins.insert(plugin.id.clone(), plugin.clone());
        let event = self.event(thread_id, None, EventKind::PluginRegistered { plugin });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_plugins(
        &self,
        params: PluginListParams,
    ) -> Result<RuntimeListResult<PluginRef>, RuntimeError> {
        let cwd = params
            .thread_id
            .as_deref()
            .map(|thread_id| {
                self.thread(thread_id)
                    .map(|thread| thread.project.cwd.clone())
            })
            .transpose()?;

        let mut items: Vec<PluginRef> = self
            .plugins
            .values()
            .cloned()
            .map(|mut plugin| {
                if plugin.scope.is_none() {
                    plugin.scope = Some(PluginScope::Runtime);
                }
                plugin
            })
            .collect();
        if let Some(cwd) = cwd.as_deref() {
            items.extend(configured_plugin_refs(Some(cwd)));
        }
        Ok(RuntimeListResult { items })
    }

    pub fn materialize_plugin_status(
        &self,
        params: PluginMaterializeParams,
    ) -> Result<PluginMaterializeResult, RuntimeError> {
        let plugins = self
            .list_plugins(PluginListParams {
                thread_id: params.thread_id.clone(),
            })?
            .items;
        let plugin = plugins
            .into_iter()
            .find(|plugin| {
                plugin.id == params.plugin_id
                    && params.scope.is_none_or(|scope| plugin.scope == Some(scope))
            })
            .ok_or_else(|| not_found("plugin", &params.plugin_id))?;
        if plugin
            .materialization
            .as_ref()
            .is_some_and(|materialization| {
                materialization.status == PluginMaterializationStatus::Materialized
            })
        {
            return Ok(PluginMaterializeResult {
                plugin,
                events: Vec::new(),
            });
        }
        Err(RuntimeError::new(
            "method_not_available_from_runtime",
            RuntimeErrorCategory::InvalidRequest,
            "plugin/materialize is a CLI-owned package materialization command; the runtime only reports already-materialized status",
        ))
    }

    pub fn set_memory_status(
        &mut self,
        params: MemorySetParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.memory = params.status.clone();
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::MemoryStatusChanged {
                status: params.status,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn memory_status(&self) -> MemoryStatus {
        self.memory.clone()
    }

    fn memory_backend_runner(&self) -> Result<Arc<dyn MemoryBackendRunner>, RuntimeError> {
        self.memory_backend_runner.clone().ok_or_else(|| {
            RuntimeError::new(
                "memory_backend_runner_unconfigured",
                RuntimeErrorCategory::InvalidRequest,
                "memory backend runner is not configured for this runtime",
            )
        })
    }

    fn start_memory_backend_job(
        &mut self,
        thread_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        message: impl Into<String>,
        dry_run: bool,
    ) -> Result<MemoryJobId, RuntimeError> {
        self.start_memory_backend_job_with_models(
            thread_id, kind, stage, message, dry_run, None, None,
        )
    }

    fn start_memory_backend_job_with_models(
        &mut self,
        thread_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        message: impl Into<String>,
        dry_run: bool,
        agent_model: Option<String>,
        deep_model: Option<String>,
    ) -> Result<MemoryJobId, RuntimeError> {
        let started = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.to_string(),
            job_id: None,
            kind,
            stage: Some(stage.to_string()),
            status: MemoryJobStatus::Running,
            source: Some("memory-backend".to_string()),
            message: Some(message.into()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: 0,
            deleted_count: 0,
            dry_run,
            backend_applied: false,
            agent_model,
            deep_model,
            model_used: None,
        })?;
        started
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::MemoryJobUpdated { job } => Some(job.id.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                RuntimeError::new(
                    "memory_backend_job_missing",
                    RuntimeErrorCategory::EventStore,
                    "memory backend execution did not produce a running job record",
                )
            })
    }

    fn finish_memory_backend_job(
        &mut self,
        thread_id: &str,
        job_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        message: impl Into<String>,
        record_count: u64,
        deleted_count: u64,
        dry_run: bool,
        backend_applied: bool,
    ) -> Result<(), RuntimeError> {
        self.finish_memory_backend_job_with_models(
            thread_id,
            job_id,
            kind,
            stage,
            message,
            record_count,
            deleted_count,
            dry_run,
            backend_applied,
            None,
        )
    }

    fn finish_memory_backend_job_with_models(
        &mut self,
        thread_id: &str,
        job_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        message: impl Into<String>,
        record_count: u64,
        deleted_count: u64,
        dry_run: bool,
        backend_applied: bool,
        model_used: Option<String>,
    ) -> Result<(), RuntimeError> {
        if self.memory_jobs.get(job_id).is_some_and(|job| {
            job.thread_id == thread_id && job.status == MemoryJobStatus::Cancelled
        }) {
            self.refresh_memory_queue_depth();
            return Ok(());
        }
        self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.to_string(),
            job_id: Some(job_id.to_string()),
            kind,
            stage: Some(stage.to_string()),
            status: MemoryJobStatus::Succeeded,
            source: Some("memory-backend".to_string()),
            message: Some(message.into()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count,
            deleted_count,
            dry_run,
            backend_applied,
            agent_model: None,
            deep_model: None,
            model_used,
        })?;
        Ok(())
    }

    fn fail_memory_backend_job(
        &mut self,
        thread_id: &str,
        job_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        message: impl Into<String>,
        dry_run: bool,
    ) {
        if self.memory_jobs.get(job_id).is_some_and(|job| {
            job.thread_id == thread_id && job.status == MemoryJobStatus::Cancelled
        }) {
            self.refresh_memory_queue_depth();
            return;
        }
        let _ = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.to_string(),
            job_id: Some(job_id.to_string()),
            kind,
            stage: Some(stage.to_string()),
            status: MemoryJobStatus::Failed,
            source: Some("memory-backend".to_string()),
            message: Some(message.into()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: 0,
            deleted_count: 0,
            dry_run,
            backend_applied: false,
            agent_model: None,
            deep_model: None,
            model_used: None,
        });
    }

    fn memory_backend_execution_result<T: Serialize>(
        &self,
        job_id: MemoryJobId,
        operation: MemoryBackendOperation,
        raw: &MemoryBackendRawResult,
        normalized: &T,
        summary: String,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let normalized = serde_json::to_value(normalized).map_err(|error| {
            RuntimeError::new(
                "memory_backend_normalization_serialize_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!("failed to serialize normalized memory backend result: {error}"),
            )
        })?;
        let cancelled = self
            .memory_jobs
            .get(&job_id)
            .is_some_and(|job| job.status == MemoryJobStatus::Cancelled);
        Ok(MemoryBackendExecutionResult {
            job_id,
            request_id: None,
            operation,
            normalized,
            backend: raw.backend.clone(),
            adapter_kind: raw.adapter_kind.clone(),
            summary: Some(summary),
            timed_out: false,
            timeout_ms: None,
            cancelled,
            diagnostics,
        })
    }

    pub fn memory_backend_status(
        &mut self,
        params: MemoryBackendStatusParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "status";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::Status,
            stage,
            "Reading Hoppi memory status through runtime backend broker.",
            false,
        )?;
        let raw = match runner.status(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Status,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let status = memory_backend_status_from_raw(&raw, &self.memory);
        let normalized = self
            .set_memory_status(MemorySetParams {
                thread_id: params.thread_id.clone(),
                status,
            })
            .map(|_| self.memory_status());
        let normalized = match normalized {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Status,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Read Hoppi memory status through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Status,
            stage,
            summary.clone(),
            normalized.memory_count,
            0,
            false,
            true,
        )?;
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Status,
            &raw,
            &normalized,
            summary,
            Vec::new(),
        )
    }

    pub fn memory_backend_recall(
        &mut self,
        params: MemoryBackendRecallParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let mut params = params;
        params.prompt = memory_backend_required_text(&params.prompt, "prompt", 16_000)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = memory_backend_recall_stage(&params).to_string();
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::Recall,
            &stage,
            "Running Hoppi memory recall through runtime backend broker.",
            false,
        )?;

        let raw = match runner.recall(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Recall,
                    &stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized_params =
            memory_recall_params_from_backend_raw(&params.thread_id, &params.prompt, &raw);
        let normalized = match self.memory_recall(normalized_params) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Recall,
                    &stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Recalled Hoppi context through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Recall,
            &stage,
            summary.clone(),
            normalized.total_count,
            0,
            false,
            true,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Recall,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_remember(
        &mut self,
        params: MemoryBackendRememberParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let params = sanitize_memory_backend_remember_params(params)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "remember";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::Remember,
            stage,
            "Saving Hoppi memory through runtime backend broker.",
            false,
        )?;
        let raw = match runner.remember(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Remember,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized =
            match self.memory_remember(memory_remember_params_from_backend_raw(&params, &raw)) {
                Ok(result) => result,
                Err(error) => {
                    self.fail_memory_backend_job(
                        &params.thread_id,
                        &job_id,
                        MemoryJobKind::Remember,
                        stage,
                        error.message.clone(),
                        false,
                    );
                    return Err(error);
                }
            };
        let summary = "Saved Hoppi memory through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Remember,
            stage,
            summary.clone(),
            normalized.record.is_some() as u64,
            0,
            false,
            normalized.backend_applied,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Remember,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_list(
        &mut self,
        params: MemoryBackendListParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let params = sanitize_memory_backend_list_params(params)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = memory_backend_list_stage(&params);
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::List,
            stage,
            "Listing Hoppi memories through runtime backend broker.",
            false,
        )?;
        let raw = match runner.list(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::List,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let records = memory_backend_records_from_raw(&raw);
        let status = memory_backend_optional_status_from_raw(&raw, &self.memory);
        if let Err(error) = self.set_memory_records(MemoryRecordsSetParams {
            thread_id: params.thread_id.clone(),
            records,
            status,
            source: Some(raw.backend.clone()),
        }) {
            self.fail_memory_backend_job(
                &params.thread_id,
                &job_id,
                MemoryJobKind::List,
                stage,
                error.message.clone(),
                false,
            );
            return Err(error);
        }
        let normalized = match self.memory_list(MemoryListParams {
            thread_id: params.thread_id.clone(),
            query: params.query.clone(),
            limit: params.limit,
            pinned_only: params.pinned_only,
            include_superseded: params.include_superseded,
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::List,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Listed Hoppi memories through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::List,
            stage,
            summary.clone(),
            normalized.items.len() as u64,
            0,
            false,
            true,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::List,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_dashboard(
        &mut self,
        params: MemoryBackendDashboardParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "dashboard-start";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::Dashboard,
            stage,
            "Starting Hoppi dashboard through runtime backend broker.",
            false,
        )?;
        let raw = match runner.dashboard(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Dashboard,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let status = memory_backend_status_from_raw(&raw, &self.memory);
        if let Err(error) = self.set_memory_status(MemorySetParams {
            thread_id: params.thread_id.clone(),
            status,
        }) {
            self.fail_memory_backend_job(
                &params.thread_id,
                &job_id,
                MemoryJobKind::Dashboard,
                stage,
                error.message.clone(),
                false,
            );
            return Err(error);
        }
        let normalized = match self.memory_dashboard(MemoryDashboardParams {
            thread_id: params.thread_id.clone(),
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Dashboard,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Started Hoppi dashboard through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Dashboard,
            stage,
            summary.clone(),
            normalized.status.memory_count,
            0,
            false,
            true,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Dashboard,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_forget(
        &mut self,
        params: MemoryBackendForgetParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let mut params = params;
        params.id = memory_backend_required_text(&params.id, "id", 512)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "forget";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::RecordMutation,
            stage,
            "Forgetting Hoppi memory through runtime backend broker.",
            false,
        )?;
        let raw = match runner.forget(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized = match self.forget_memory_record(MemoryRecordForgetParams {
            thread_id: params.thread_id.clone(),
            id: params.id.clone(),
            backend_applied: true,
            status: memory_backend_optional_status_from_raw(&raw, &self.memory),
            source: Some(raw.backend.clone()),
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Forgot Hoppi memory through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::RecordMutation,
            stage,
            summary.clone(),
            0,
            normalized.cache_updated as u64,
            false,
            normalized.backend_applied,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Forget,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_pin(
        &mut self,
        params: MemoryBackendPinParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let mut params = params;
        params.id = memory_backend_required_text(&params.id, "id", 512)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = if params.pinned { "pin" } else { "unpin" };
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::RecordMutation,
            stage,
            "Updating Hoppi memory pin through runtime backend broker.",
            false,
        )?;
        let raw = match runner.pin(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized = match self.pin_memory_record(MemoryRecordPinParams {
            thread_id: params.thread_id.clone(),
            id: params.id.clone(),
            pinned: params.pinned,
            backend_applied: true,
            record: memory_backend_record_from_raw(&raw),
            status: memory_backend_optional_status_from_raw(&raw, &self.memory),
            source: Some(raw.backend.clone()),
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Updated Hoppi memory pin through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::RecordMutation,
            stage,
            summary.clone(),
            normalized.record.is_some() as u64,
            0,
            false,
            normalized.backend_applied,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Pin,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_update(
        &mut self,
        params: MemoryBackendUpdateParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let mut params = params;
        params.id = memory_backend_required_text(&params.id, "id", 512)?;
        params.patch = sanitize_memory_backend_patch(params.patch)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "edit";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::RecordMutation,
            stage,
            "Updating Hoppi memory through runtime backend broker.",
            false,
        )?;
        let raw = match runner.update(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized = match self.update_memory_record(MemoryRecordUpdateParams {
            thread_id: params.thread_id.clone(),
            id: params.id.clone(),
            patch: params.patch.clone(),
            backend_applied: true,
            record: memory_backend_record_from_raw(&raw),
            status: memory_backend_optional_status_from_raw(&raw, &self.memory),
            source: Some(raw.backend.clone()),
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Updated Hoppi memory through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::RecordMutation,
            stage,
            summary.clone(),
            normalized.record.is_some() as u64,
            0,
            false,
            normalized.backend_applied,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Update,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_supersede(
        &mut self,
        params: MemoryBackendSupersedeParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let mut params = params;
        params.id = memory_backend_required_text(&params.id, "id", 512)?;
        params.superseded_by =
            memory_backend_required_text(&params.superseded_by, "supersededBy", 512)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = "supersede";
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::RecordMutation,
            stage,
            "Superseding Hoppi memory through runtime backend broker.",
            false,
        )?;
        let raw = match runner.supersede(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let normalized = match self.supersede_memory_record(MemoryRecordSupersedeParams {
            thread_id: params.thread_id.clone(),
            id: params.id.clone(),
            superseded_by: params.superseded_by.clone(),
            backend_applied: true,
            record: memory_backend_record_from_raw(&raw),
            status: memory_backend_optional_status_from_raw(&raw, &self.memory),
            source: Some(raw.backend.clone()),
        }) {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::RecordMutation,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let summary = "Superseded Hoppi memory through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::RecordMutation,
            stage,
            summary.clone(),
            normalized.record.is_some() as u64,
            0,
            false,
            normalized.backend_applied,
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Supersede,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_consolidate(
        &mut self,
        params: MemoryBackendConsolidateParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let params = sanitize_memory_backend_consolidate_params(params)?;
        let thread = self.thread(&params.thread_id)?.clone();
        let runner = self.memory_backend_runner()?;
        let stage = params
            .stage
            .as_deref()
            .filter(|stage| !stage.is_empty())
            .unwrap_or("manual-maintenance")
            .to_string();
        let job_id = self.start_memory_backend_job_with_models(
            &params.thread_id,
            MemoryJobKind::Consolidate,
            &stage,
            "Consolidating Hoppi memories through runtime backend broker.",
            params.dry_run,
            params.agent_model.clone(),
            params.deep_model.clone(),
        )?;
        let raw = match runner.consolidate(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Consolidate,
                    &stage,
                    error.message.clone(),
                    params.dry_run,
                );
                return Err(error);
            }
        };
        let normalized = match self
            .memory_consolidate(memory_consolidate_params_from_backend_raw(&params, &raw))
        {
            Ok(result) => result,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Consolidate,
                    &stage,
                    error.message.clone(),
                    params.dry_run,
                );
                return Err(error);
            }
        };
        let summary = "Consolidated Hoppi memories through runtime backend broker.".to_string();
        self.finish_memory_backend_job_with_models(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Consolidate,
            &stage,
            summary.clone(),
            normalized.created_count,
            normalized.deleted_count,
            normalized.dry_run,
            normalized.backend_applied,
            normalized.model_used.clone(),
        )?;
        let diagnostics = normalized.diagnostics.clone();
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Consolidate,
            &raw,
            &normalized,
            summary,
            diagnostics,
        )
    }

    pub fn memory_backend_sync(
        &mut self,
        params: MemoryBackendSyncParams,
    ) -> Result<MemoryBackendExecutionResult, RuntimeError> {
        let params = sanitize_memory_backend_sync_params(params)?;
        self.thread(&params.thread_id)?;
        let runner = self.memory_backend_runner()?;
        let thread = self.thread(&params.thread_id)?.clone();
        let stage = memory_backend_sync_stage(params.direction);
        let job_id = self.start_memory_backend_job(
            &params.thread_id,
            MemoryJobKind::Sync,
            stage,
            "Syncing Hoppi memories through runtime backend broker.",
            false,
        )?;
        let raw = match runner.sync(&thread, &params) {
            Ok(raw) => raw,
            Err(error) => {
                self.fail_memory_backend_job(
                    &params.thread_id,
                    &job_id,
                    MemoryJobKind::Sync,
                    stage,
                    error.message.clone(),
                    false,
                );
                return Err(error);
            }
        };
        let record_count = memory_backend_sync_record_count(&raw.value);
        let summary = "Synced Hoppi memories through runtime backend broker.".to_string();
        self.finish_memory_backend_job(
            &params.thread_id,
            &job_id,
            MemoryJobKind::Sync,
            stage,
            summary.clone(),
            record_count,
            0,
            false,
            true,
        )?;
        self.memory_backend_execution_result(
            job_id,
            MemoryBackendOperation::Sync,
            &raw,
            &raw.value,
            summary,
            Vec::new(),
        )
    }

    fn next_memory_job_id(&mut self) -> MemoryJobId {
        self.next_memory_job += 1;
        format!("mem-job-{}", self.next_memory_job)
    }

    fn bump_memory_job_counter(&mut self, id: &str) {
        if let Some(number) = id.strip_prefix("mem-job-")
            && let Ok(number) = number.parse::<u64>()
        {
            self.next_memory_job = self.next_memory_job.max(number);
        }
    }

    fn memory_queue_depth(&self) -> u64 {
        self.memory_jobs
            .values()
            .filter(|job| job.status.is_active())
            .count() as u64
    }

    fn refresh_memory_queue_depth(&mut self) -> u64 {
        let queue_depth = self.memory_queue_depth();
        self.memory.queue_depth = Some(queue_depth);
        queue_depth
    }

    fn memory_job_controls() -> Vec<MemoryControl> {
        vec![
            MemoryControl {
                id: "memory-status".to_string(),
                label: "Memory status".to_string(),
                command: "/memory status".to_string(),
                description: Some("Show current Hoppi memory status.".to_string()),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Open the Hoppi dashboard for detailed job context.".to_string()),
            },
        ]
    }

    pub fn list_memory_jobs(
        &mut self,
        params: MemoryJobsListParams,
    ) -> Result<MemoryJobsResult, RuntimeError> {
        if let Some(thread_id) = params.thread_id.as_deref() {
            self.thread(thread_id)?;
        }
        let queue_depth = self.refresh_memory_queue_depth();
        let mut items = self
            .memory_jobs
            .values()
            .filter(|job| {
                params
                    .thread_id
                    .as_ref()
                    .is_none_or(|thread_id| &job.thread_id == thread_id)
            })
            .filter(|job| params.include_finished || job.status.is_active())
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.status
                .is_active()
                .cmp(&right.status.is_active())
                .reverse()
                .then_with(|| left.id.cmp(&right.id))
        });
        let total_count = items.len() as u64;
        if let Some(limit) = params.limit {
            items.truncate(limit.min(200) as usize);
        }
        let message = if queue_depth == 0 {
            format!(
                "Runtime memory worker has no active jobs and {} visible job{}.",
                total_count,
                if total_count == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "Runtime memory worker has {queue_depth} active job{}.",
                if queue_depth == 1 { "" } else { "s" }
            )
        };
        Ok(MemoryJobsResult {
            status: self.memory.clone(),
            queue_depth,
            total_count,
            items,
            message,
            controls: Self::memory_job_controls(),
            diagnostics: Vec::new(),
            events: Vec::new(),
        })
    }

    pub fn update_memory_job(
        &mut self,
        params: MemoryJobUpdateParams,
    ) -> Result<MemoryJobsResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params
            .job_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.next_memory_job_id());
        self.bump_memory_job_counter(&id);
        let previous = self.memory_jobs.get(&id);
        let job = MemoryJobRef {
            id: id.clone(),
            thread_id: params.thread_id.clone(),
            kind: params.kind,
            stage: params
                .stage
                .as_deref()
                .map(str::trim)
                .filter(|stage| !stage.is_empty())
                .map(str::to_string)
                .or_else(|| previous.and_then(|job| job.stage.clone())),
            status: params.status,
            source: params
                .source
                .as_deref()
                .map(str::trim)
                .filter(|source| !source.is_empty())
                .map(str::to_string)
                .or_else(|| previous.and_then(|job| job.source.clone())),
            message: params
                .message
                .as_deref()
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string),
            queued_at: params
                .queued_at
                .or_else(|| previous.and_then(|job| job.queued_at.clone())),
            started_at: params
                .started_at
                .or_else(|| previous.and_then(|job| job.started_at.clone())),
            finished_at: params
                .finished_at
                .or_else(|| previous.and_then(|job| job.finished_at.clone())),
            record_count: params.record_count,
            deleted_count: params.deleted_count,
            dry_run: params.dry_run,
            backend_applied: params.backend_applied,
            agent_model: memory_backend_optional_text(params.agent_model, 128)
                .or_else(|| previous.and_then(|job| job.agent_model.clone())),
            deep_model: memory_backend_optional_text(params.deep_model, 128)
                .or_else(|| previous.and_then(|job| job.deep_model.clone())),
            model_used: memory_backend_optional_text(params.model_used, 128)
                .or_else(|| previous.and_then(|job| job.model_used.clone())),
        };
        self.memory_jobs.insert(id, job.clone());
        let queue_depth = self.refresh_memory_queue_depth();
        let event = self.event(&params.thread_id, None, EventKind::MemoryJobUpdated { job });
        let mut result = self.list_memory_jobs(MemoryJobsListParams {
            thread_id: Some(params.thread_id),
            limit: Some(50),
            include_finished: true,
        })?;
        result.queue_depth = queue_depth;
        result.status = self.memory.clone();
        result.events = vec![event];
        Ok(result)
    }

    pub fn cancel_memory_job(
        &mut self,
        params: MemoryJobCancelParams,
    ) -> Result<MemoryJobsResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params.job_id.trim();
        if id.is_empty() {
            return Err(RuntimeError::new(
                "memory_job_id_required",
                RuntimeErrorCategory::InvalidRequest,
                "memory job cancellation requires a job id",
            ));
        }
        let previous = self
            .memory_jobs
            .get(id)
            .filter(|job| job.thread_id == params.thread_id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    "memory_job_not_found",
                    RuntimeErrorCategory::NotFound,
                    format!("memory job '{id}' was not found for this thread"),
                )
            })?;
        if !previous.status.is_active() {
            return Err(RuntimeError::new(
                "memory_job_not_active",
                RuntimeErrorCategory::TerminalState,
                format!("memory job '{id}' is already {:?}", previous.status),
            ));
        }
        let message = params
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Cancelled memory job {id}."));
        let job = MemoryJobRef {
            status: MemoryJobStatus::Cancelled,
            message: Some(message),
            finished_at: params
                .finished_at
                .as_deref()
                .map(str::trim)
                .filter(|finished_at| !finished_at.is_empty())
                .map(str::to_string)
                .or(previous.finished_at),
            backend_applied: false,
            ..previous
        };
        self.memory_jobs.insert(id.to_string(), job.clone());
        let queue_depth = self.refresh_memory_queue_depth();
        let event = self.event(&params.thread_id, None, EventKind::MemoryJobUpdated { job });
        let mut result = self.list_memory_jobs(MemoryJobsListParams {
            thread_id: Some(params.thread_id),
            limit: Some(50),
            include_finished: true,
        })?;
        result.queue_depth = queue_depth;
        result.status = self.memory.clone();
        result.events = vec![event];
        Ok(result)
    }

    pub fn set_memory_records(
        &mut self,
        params: MemoryRecordsSetParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.memory_records = Some(params.records.clone());
        if let Some(status) = params.status {
            self.memory = status;
        } else {
            self.memory.memory_count = params.records.len() as u64;
            self.memory.pinned_count = params
                .records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("recordCount".to_string(), params.records.len().to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory records cached".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn memory_list(
        &mut self,
        params: MemoryListParams,
    ) -> Result<MemoryListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_string);
        let cached = self.memory_records.as_ref();
        let available = cached.is_some();
        let mut items = cached
            .into_iter()
            .flat_map(|records| records.iter())
            .filter(|record| params.include_superseded || record.superseded_by.is_none())
            .filter(|record| !params.pinned_only || record.pinned || record.starred)
            .filter(|record| {
                query
                    .as_deref()
                    .is_none_or(|query| memory_record_matches_query(record, query))
            })
            .cloned()
            .collect::<Vec<_>>();
        let total_count = items.len() as u64;
        if let Some(limit) = params.limit {
            items.truncate(limit.min(500) as usize);
        }
        let message = if available {
            format!(
                "Runtime memory list contains {} cached record{}.",
                total_count,
                if total_count == 1 { "" } else { "s" }
            )
        } else {
            "No cached Hoppi records are available in Rust runtime state; run `/memory list` from a native shell host to refresh it.".to_string()
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some(
                    "Ask the native host Hoppi bridge to refresh Rust's cached memory records."
                        .to_string(),
                ),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Open the Hoppi dashboard for full CRUD actions.".to_string()),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("cached".to_string(), available.to_string());
        metadata.insert("totalCount".to_string(), total_count.to_string());
        metadata.insert("itemCount".to_string(), items.len().to_string());
        metadata.insert("pinnedOnly".to_string(), params.pinned_only.to_string());
        metadata.insert(
            "includeSuperseded".to_string(),
            params.include_superseded.to_string(),
        );
        if let Some(query) = &query {
            metadata.insert("query".to_string(), query.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory list status".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryListResult {
            status: self.memory.clone(),
            available,
            query,
            pinned_only: params.pinned_only,
            include_superseded: params.include_superseded,
            total_count,
            items,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn memory_recall(
        &mut self,
        params: MemoryRecallParams,
    ) -> Result<MemoryRecallResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_string);
        let context_markdown = params
            .context_markdown
            .as_deref()
            .map(str::trim)
            .filter(|context| !context.is_empty())
            .map(str::to_string);
        let omitted_reason = params
            .omitted_reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_string);
        let items = params
            .records
            .into_iter()
            .filter_map(|mut record| {
                record.id = record.id.trim().to_string();
                record.content = record.content.trim().to_string();
                (!record.id.is_empty() && !record.content.is_empty()).then_some(record)
            })
            .collect::<Vec<_>>();

        let mut cache_updated = false;
        if !items.is_empty() {
            let records = self.memory_records.get_or_insert_with(Vec::new);
            for record in items.iter().cloned() {
                if let Some(existing) = records.iter_mut().find(|cached| cached.id == record.id) {
                    *existing = record;
                } else {
                    records.push(record);
                }
            }
            cache_updated = true;
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }

        let context_bytes = context_markdown
            .as_ref()
            .map(|context| context.as_bytes().len() as u64)
            .unwrap_or(0);
        let total_count = items.len() as u64;
        let available = self.memory.enabled
            || params.backend_queried
            || context_markdown.is_some()
            || omitted_reason.is_some()
            || !items.is_empty();
        let message = if context_markdown.is_some() && total_count > 0 {
            format!(
                "Recalled Hoppi context for this turn and normalized {} record{} into Rust state.",
                total_count,
                if total_count == 1 { "" } else { "s" }
            )
        } else if context_markdown.is_some() {
            "Recalled Hoppi context for this turn; no records were reported for Rust cache."
                .to_string()
        } else if let Some(reason) = &omitted_reason {
            format!("Hoppi recall returned no context: {reason}")
        } else if params.backend_queried {
            "Hoppi recall completed without usable context.".to_string()
        } else {
            "No Hoppi recall result was reported; runtime memory state is unchanged.".to_string()
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some(
                    "Ask the native host Hoppi bridge to refresh Rust's cached memory records."
                        .to_string(),
                ),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Open the Hoppi dashboard for full CRUD actions.".to_string()),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert(
            "backendQueried".to_string(),
            params.backend_queried.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert("recordCount".to_string(), total_count.to_string());
        metadata.insert("contextBytes".to_string(), context_bytes.to_string());
        metadata.insert("truncated".to_string(), params.truncated.to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        if let Some(query) = &query {
            metadata.insert("query".to_string(), query.clone());
        }
        if let Some(reason) = &omitted_reason {
            metadata.insert("omittedReason".to_string(), reason.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory recall recorded".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRecallResult {
            status: self.memory.clone(),
            available,
            backend_queried: params.backend_queried,
            query,
            context_markdown,
            omitted_reason,
            context_bytes,
            truncated: params.truncated,
            total_count,
            items,
            cache_updated,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn memory_distill_turn(
        &mut self,
        params: MemoryDistillTurnParams,
    ) -> Result<MemoryDistillTurnResult, RuntimeError> {
        let thread_id = params.thread_id.clone();
        self.thread(&thread_id)?;
        let user_text = memory_backend_required_text(&params.user_text, "user_text", 4_000)?;
        let assistant_text =
            memory_backend_required_text(&params.assistant_text, "assistant_text", 6_000)?;
        let source_project = memory_backend_optional_text(params.source_project, 4096);
        let source_session_id = memory_backend_optional_text(params.source_session_id, 512);
        let recent_user_texts = memory_recurring_texts(params.recent_user_texts, 8, 512);
        let recent_assistant_texts = memory_recurring_texts(params.recent_assistant_texts, 8, 512);
        let agent_model = memory_backend_optional_text(params.agent_model, 128);
        let mut provider_config = params.model_provider;
        provider_config.system_prompt = Some(MEMORY_DISTILLER_PROMPT.to_string());
        provider_config.reasoning_effort = Some(
            provider_config
                .reasoning_effort
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("minimal")
                .to_string(),
        );
        provider_config.max_output_tokens = Some(
            provider_config
                .max_output_tokens
                .unwrap_or(MEMORY_DISTILLER_MAX_OUTPUT_TOKENS),
        );
        provider_config.stream = false;
        let model_used = provider_config.model.trim().to_string();

        let started = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.clone(),
            job_id: None,
            kind: MemoryJobKind::Remember,
            stage: Some("turn-distillation".to_string()),
            status: MemoryJobStatus::Running,
            source: Some("memory-distiller".to_string()),
            message: Some("Distilling completed turn memory through model provider.".to_string()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: 0,
            deleted_count: 0,
            dry_run: false,
            backend_applied: false,
            agent_model: agent_model.clone(),
            deep_model: None,
            model_used: None,
        })?;
        let job_id = started
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::MemoryJobUpdated { job } => Some(job.id.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                RuntimeError::new(
                    "memory_distill_job_missing",
                    RuntimeErrorCategory::EventStore,
                    "turn memory distillation did not produce a running job record",
                )
            })?;
        let mut events = started.events;
        let mut diagnostics = Vec::new();

        let mut provider =
            match model_provider(Some(provider_config.clone()), Vec::new(), Vec::new(), None) {
                Ok(provider) => provider,
                Err(error) => {
                    self.fail_memory_distill_job(
                        &thread_id,
                        &job_id,
                        error.message.clone(),
                        agent_model,
                    );
                    return Err(error);
                }
            };
        let input = memory_distiller_input(&user_text, &assistant_text);
        let request = ModelRequest {
            thread_id: thread_id.clone(),
            turn_id: job_id.clone(),
            input,
            continuation: 0,
            history: Vec::new(),
        };
        let step = match provider.next_step(&request) {
            Ok(step) => step,
            Err(error) => {
                self.fail_memory_distill_job(
                    &thread_id,
                    &job_id,
                    error.message.clone(),
                    agent_model,
                );
                return Err(error);
            }
        };
        diagnostics.extend(step.diagnostics);
        let raw = step.step.assistant_deltas.join("\n");
        let distilled = match parse_memory_distiller_output(&raw) {
            Ok(distilled) => distilled,
            Err(error) => {
                self.fail_memory_distill_job(
                    &thread_id,
                    &job_id,
                    error.message.clone(),
                    agent_model,
                );
                return Err(error);
            }
        };

        let mut remembered = false;
        let mut record = None;
        if let Some(candidate) = memory_distilled_turn_record(
            &distilled,
            source_session_id.as_deref(),
            source_project.as_deref(),
        ) {
            let remembered_result = self.memory_remember(MemoryRememberParams {
                thread_id: thread_id.clone(),
                content: candidate.content.clone(),
                tags: candidate.tags.clone(),
                layer: candidate.layer.clone(),
                confidence: candidate.confidence.clone(),
                source_session_id: source_session_id.clone(),
                backend_applied: false,
                record: Some(candidate),
                status: None,
                reason: Some("turn-distillation".to_string()),
                source: Some("oppi:turn-distillation".to_string()),
            })?;
            remembered = remembered_result.record.is_some();
            record = remembered_result.record;
            diagnostics.extend(remembered_result.diagnostics);
            events.extend(remembered_result.events);
        }

        let recurring_result =
            self.memory_recurring_candidates(MemoryRecurringCandidatesParams {
                thread_id: thread_id.clone(),
                candidates: distilled.recurring_tasks,
                source_project,
                source_session_id,
                recent_user_texts,
                recent_assistant_texts,
            })?;
        let recurring_saved_count = recurring_result.saved_count;
        let recurring_records = recurring_result.records;
        diagnostics.extend(recurring_result.diagnostics);
        events.extend(recurring_result.events);

        let record_count = remembered as u64 + recurring_saved_count;
        let finish_message = memory_distill_turn_message(remembered, recurring_saved_count);
        let finished = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.clone(),
            job_id: Some(job_id),
            kind: MemoryJobKind::Remember,
            stage: Some("turn-distillation".to_string()),
            status: MemoryJobStatus::Succeeded,
            source: Some("memory-distiller".to_string()),
            message: Some(finish_message.clone()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count,
            deleted_count: 0,
            dry_run: false,
            backend_applied: false,
            agent_model,
            deep_model: None,
            model_used: Some(model_used.clone()),
        })?;
        events.extend(finished.events);

        Ok(MemoryDistillTurnResult {
            status: self.memory.clone(),
            available: remembered || recurring_saved_count > 0,
            remembered,
            record,
            recurring_saved_count,
            recurring_records,
            model_used: Some(model_used),
            message: finish_message,
            controls: Self::memory_job_controls(),
            diagnostics,
            events,
        })
    }

    fn fail_memory_distill_job(
        &mut self,
        thread_id: &str,
        job_id: &str,
        message: String,
        agent_model: Option<String>,
    ) {
        let _ = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.to_string(),
            job_id: Some(job_id.to_string()),
            kind: MemoryJobKind::Remember,
            stage: Some("turn-distillation".to_string()),
            status: MemoryJobStatus::Failed,
            source: Some("memory-distiller".to_string()),
            message: Some(message),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: 0,
            deleted_count: 0,
            dry_run: false,
            backend_applied: false,
            agent_model,
            deep_model: None,
            model_used: None,
        });
    }

    pub fn memory_recurring_candidates(
        &mut self,
        params: MemoryRecurringCandidatesParams,
    ) -> Result<MemoryRecurringCandidatesResult, RuntimeError> {
        let thread_id = params.thread_id.clone();
        self.thread(&thread_id)?;
        let source_project = memory_backend_optional_text(params.source_project, 4096);
        let source_session_id = memory_backend_optional_text(params.source_session_id, 512);
        let recent_user_texts = memory_recurring_texts(params.recent_user_texts, 8, 512);
        let recent_assistant_texts = memory_recurring_texts(params.recent_assistant_texts, 8, 512);
        let records = params
            .candidates
            .into_iter()
            .filter_map(|candidate| {
                recurring_task_candidate_record(
                    candidate,
                    source_project.as_deref(),
                    source_session_id.as_deref(),
                    &recent_user_texts,
                    &recent_assistant_texts,
                )
            })
            .take(RECURRING_TASK_MAX_CANDIDATES)
            .collect::<Vec<_>>();

        let started = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.clone(),
            job_id: None,
            kind: MemoryJobKind::Remember,
            stage: Some("recurring-task-mining".to_string()),
            status: MemoryJobStatus::Running,
            source: Some("memory-recurring-task-mining".to_string()),
            message: Some("Mining recurring task candidates from turn memory.".to_string()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: 0,
            deleted_count: 0,
            dry_run: false,
            backend_applied: false,
            agent_model: None,
            deep_model: None,
            model_used: None,
        })?;
        let job_id = started
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::MemoryJobUpdated { job } => Some(job.id.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                RuntimeError::new(
                    "memory_recurring_job_missing",
                    RuntimeErrorCategory::EventStore,
                    "recurring task mining did not produce a running job record",
                )
            })?;
        let mut events = started.events;
        let mut diagnostics = Vec::new();

        for record in &records {
            let remembered = self.memory_remember(MemoryRememberParams {
                thread_id: thread_id.clone(),
                content: record.content.clone(),
                tags: record.tags.clone(),
                layer: record.layer.clone(),
                confidence: record.confidence.clone(),
                source_session_id: source_session_id.clone(),
                backend_applied: false,
                record: Some(record.clone()),
                status: None,
                reason: Some("recurring-task-mining".to_string()),
                source: record.source.clone(),
            })?;
            diagnostics.extend(remembered.diagnostics);
            events.extend(remembered.events);
        }

        let saved_count = records.len() as u64;
        let finish_status = if saved_count == 0 {
            MemoryJobStatus::Skipped
        } else {
            MemoryJobStatus::Succeeded
        };
        let finish_message = if saved_count == 0 {
            "No recurring task candidates met the recording threshold.".to_string()
        } else {
            format!(
                "Recorded {saved_count} recurring task candidate{}.",
                if saved_count == 1 { "" } else { "s" }
            )
        };
        let finished = self.update_memory_job(MemoryJobUpdateParams {
            thread_id: thread_id.clone(),
            job_id: Some(job_id),
            kind: MemoryJobKind::Remember,
            stage: Some("recurring-task-mining".to_string()),
            status: finish_status,
            source: Some("memory-recurring-task-mining".to_string()),
            message: Some(finish_message.clone()),
            queued_at: None,
            started_at: None,
            finished_at: None,
            record_count: saved_count,
            deleted_count: 0,
            dry_run: false,
            backend_applied: false,
            agent_model: None,
            deep_model: None,
            model_used: None,
        })?;
        events.extend(finished.events);

        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("savedCount".to_string(), saved_count.to_string());
        metadata.insert(
            "source".to_string(),
            "memory-recurring-task-mining".to_string(),
        );
        if let Some(source_project) = &source_project {
            metadata.insert("sourceProject".to_string(), source_project.clone());
        }
        if let Some(source_session_id) = &source_session_id {
            metadata.insert("sourceSessionId".to_string(), source_session_id.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "recurring task candidates recorded".to_string(),
            metadata,
        };
        let diagnostic_event = self.event(
            &thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        diagnostics.push(diagnostic);
        events.push(diagnostic_event);

        Ok(MemoryRecurringCandidatesResult {
            status: self.memory.clone(),
            available: self.memory.enabled || saved_count > 0,
            saved_count,
            records,
            message: finish_message,
            controls: Self::memory_job_controls(),
            diagnostics,
            events,
        })
    }

    pub fn memory_remember(
        &mut self,
        params: MemoryRememberParams,
    ) -> Result<MemoryRememberResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let content = params.content.trim().to_string();
        let record_content = params
            .record
            .as_ref()
            .map(|record| record.content.trim().to_string())
            .filter(|content| !content.is_empty());
        let effective_content = if content.is_empty() {
            record_content.clone().unwrap_or_default()
        } else {
            content.clone()
        };
        if effective_content.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_remember_content",
                RuntimeErrorCategory::InvalidRequest,
                "memory remember content cannot be empty",
            ));
        }

        let tags = params
            .tags
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        let layer = params
            .layer
            .as_deref()
            .map(str::trim)
            .filter(|layer| !layer.is_empty())
            .map(str::to_string);
        let confidence = params
            .confidence
            .as_deref()
            .map(str::trim)
            .filter(|confidence| !confidence.is_empty())
            .map(str::to_string);
        let source_session_id = params
            .source_session_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let reason = params
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_string);
        let source = params
            .source
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty())
            .map(str::to_string);
        let mut incoming = params.record.map(|mut record| {
            record.id = record.id.trim().to_string();
            record.content = record.content.trim().to_string();
            if record.content.is_empty() {
                record.content = effective_content.clone();
            }
            if record.tags.is_empty() {
                record.tags = tags.clone();
            }
            if record.layer.is_none() {
                record.layer = layer.clone();
            }
            if record.confidence.is_none() {
                record.confidence = confidence.clone();
            }
            record
        });
        if let Some(record) = &incoming
            && (record.id.is_empty() || record.content.is_empty())
        {
            incoming = None;
        }

        let mut cache_updated = false;
        let mut result_record = None;
        if let Some(record) = incoming {
            let records = self.memory_records.get_or_insert_with(Vec::new);
            if let Some(existing) = records.iter_mut().find(|cached| cached.id == record.id) {
                *existing = record.clone();
            } else {
                records.push(record.clone());
            }
            result_record = Some(record);
            cache_updated = true;
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        } else if params.backend_applied {
            self.memory.enabled = true;
            self.memory.initialized = true;
            if self.memory.backend == "none" {
                self.memory.backend = "hoppi".to_string();
            }
        }

        let saved = params.backend_applied || cache_updated;
        let available = self.memory.enabled || saved;
        let record_id = result_record.as_ref().map(|record| record.id.clone());
        let message = match (params.backend_applied, cache_updated, record_id.as_deref()) {
            (true, true, Some(id)) => {
                format!("Saved Hoppi memory record {id} and updated Rust cache.")
            }
            (true, true, None) => "Saved Hoppi memory and updated Rust cache.".to_string(),
            (true, false, _) => {
                "Saved Hoppi memory through the backend; no record was reported for Rust cache."
                    .to_string()
            }
            (false, true, Some(id)) => {
                format!(
                    "Recorded memory candidate {id} in Rust cache; backend write was not reported."
                )
            }
            (false, false, _) => {
                "No Hoppi memory backend write or cacheable record was reported.".to_string()
            }
            (false, true, None) => {
                "Recorded memory candidate in Rust cache; backend write was not reported."
                    .to_string()
            }
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some(
                    "Ask the native host Hoppi bridge to refresh Rust's cached memory records."
                        .to_string(),
                ),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Open the Hoppi dashboard for full CRUD actions.".to_string()),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert("saved".to_string(), saved.to_string());
        metadata.insert(
            "contentBytes".to_string(),
            effective_content.as_bytes().len().to_string(),
        );
        metadata.insert("tagCount".to_string(), tags.len().to_string());
        metadata.insert(
            "source".to_string(),
            source.clone().unwrap_or_else(|| "client".to_string()),
        );
        if let Some(reason) = &reason {
            metadata.insert("reason".to_string(), reason.clone());
        }
        if let Some(source_session_id) = &source_session_id {
            metadata.insert("sourceSessionId".to_string(), source_session_id.clone());
        }
        if let Some(record_id) = &record_id {
            metadata.insert("recordId".to_string(), record_id.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory remember recorded".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRememberResult {
            status: self.memory.clone(),
            available,
            saved,
            backend_applied: params.backend_applied,
            cache_updated,
            record: result_record,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn forget_memory_record(
        &mut self,
        params: MemoryRecordForgetParams,
    ) -> Result<MemoryRecordMutationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params.id.trim().to_string();
        if id.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_record_id",
                RuntimeErrorCategory::InvalidRequest,
                "memory record id cannot be empty",
            ));
        }

        let mut removed = None;
        if let Some(records) = self.memory_records.as_mut()
            && let Some(index) = records.iter().position(|record| record.id == id)
        {
            removed = Some(records.remove(index));
        }
        let cache_updated = removed.is_some();

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }

        let message = match (params.backend_applied, cache_updated) {
            (true, true) => {
                format!("Forgot memory record {id} and removed it from Rust cache.")
            }
            (true, false) => {
                format!(
                    "Forgot memory record {id} in the backend; no matching cached record was present in Rust."
                )
            }
            (false, true) => {
                format!(
                    "Removed memory record {id} from Rust cache; backend mutation was not reported."
                )
            }
            (false, false) => {
                format!(
                    "No cached memory record {id} was present; backend mutation was not reported."
                )
            }
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some("Refresh cached Hoppi records after mutation.".to_string()),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some(
                    "Open the Hoppi dashboard for detailed CRUD actions.".to_string(),
                ),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("action".to_string(), "forget".to_string());
        metadata.insert("id".to_string(), id.clone());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory record forgotten".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRecordMutationResult {
            status: self.memory.clone(),
            action: "forget".to_string(),
            id,
            backend_applied: params.backend_applied,
            cache_updated,
            record: removed,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn update_memory_record(
        &mut self,
        params: MemoryRecordUpdateParams,
    ) -> Result<MemoryRecordMutationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params.id.trim().to_string();
        if id.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_record_id",
                RuntimeErrorCategory::InvalidRequest,
                "memory record id cannot be empty",
            ));
        }

        let mut incoming = params.record.clone().map(|mut record| {
            if record.id.trim().is_empty() {
                record.id = id.clone();
            }
            apply_memory_record_patch(&mut record, &params.patch);
            record
        });
        if let Some(record) = &incoming
            && record.id != id
        {
            return Err(RuntimeError::new(
                "memory_record_id_mismatch",
                RuntimeErrorCategory::InvalidRequest,
                format!(
                    "memory record id {} does not match requested id {id}",
                    record.id
                ),
            ));
        }
        if incoming.is_none() && memory_record_patch_is_empty(&params.patch) {
            return Err(RuntimeError::new(
                "invalid_memory_record_patch",
                RuntimeErrorCategory::InvalidRequest,
                "memory record update patch cannot be empty",
            ));
        }

        let mut cache_updated = false;
        let mut result_record = None;
        if let Some(records) = self.memory_records.as_mut() {
            if let Some(index) = records.iter().position(|record| record.id == id) {
                if let Some(record) = incoming.take() {
                    records[index] = record;
                } else {
                    apply_memory_record_patch(&mut records[index], &params.patch);
                }
                result_record = Some(records[index].clone());
                cache_updated = true;
            } else if let Some(record) = incoming.take() {
                records.push(record.clone());
                result_record = Some(record);
                cache_updated = true;
            }
        } else if let Some(record) = incoming.take() {
            self.memory_records = Some(vec![record.clone()]);
            result_record = Some(record);
            cache_updated = true;
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }

        let message = match (params.backend_applied, cache_updated) {
            (true, true) => {
                format!("Updated memory record {id} and updated Rust cache.")
            }
            (true, false) => {
                format!(
                    "Updated memory record {id} in the backend; no matching cached record was present in Rust."
                )
            }
            (false, true) => {
                format!(
                    "Updated memory record {id} in Rust cache; backend mutation was not reported."
                )
            }
            (false, false) => {
                format!(
                    "No cached memory record {id} was present; backend update was not reported."
                )
            }
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some("Refresh cached Hoppi records after mutation.".to_string()),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some(
                    "Open the Hoppi dashboard for detailed CRUD actions.".to_string(),
                ),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("action".to_string(), "update".to_string());
        metadata.insert("id".to_string(), id.clone());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory record updated".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRecordMutationResult {
            status: self.memory.clone(),
            action: "update".to_string(),
            id,
            backend_applied: params.backend_applied,
            cache_updated,
            record: result_record,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn supersede_memory_record(
        &mut self,
        params: MemoryRecordSupersedeParams,
    ) -> Result<MemoryRecordMutationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params.id.trim().to_string();
        let superseded_by = params.superseded_by.trim().to_string();
        if id.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_record_id",
                RuntimeErrorCategory::InvalidRequest,
                "memory record id cannot be empty",
            ));
        }
        if superseded_by.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_record_superseded_by",
                RuntimeErrorCategory::InvalidRequest,
                "superseding memory record id cannot be empty",
            ));
        }
        if id == superseded_by {
            return Err(RuntimeError::new(
                "invalid_memory_record_supersede_cycle",
                RuntimeErrorCategory::InvalidRequest,
                "memory record cannot supersede itself",
            ));
        }

        let mut incoming = params.record.clone().map(|mut record| {
            if record.id.trim().is_empty() {
                record.id = id.clone();
            }
            record.superseded_by = Some(superseded_by.clone());
            record
        });
        if let Some(record) = &incoming
            && record.id != id
        {
            return Err(RuntimeError::new(
                "memory_record_id_mismatch",
                RuntimeErrorCategory::InvalidRequest,
                format!(
                    "memory record id {} does not match requested id {id}",
                    record.id
                ),
            ));
        }

        let mut cache_updated = false;
        let mut result_record = None;
        if let Some(records) = self.memory_records.as_mut() {
            if let Some(index) = records.iter().position(|record| record.id == id) {
                if let Some(record) = incoming.take() {
                    records[index] = record;
                } else {
                    records[index].superseded_by = Some(superseded_by.clone());
                }
                result_record = Some(records[index].clone());
                cache_updated = true;
            } else if let Some(record) = incoming.take() {
                records.push(record.clone());
                result_record = Some(record);
                cache_updated = true;
            }
        } else if let Some(record) = incoming.take() {
            self.memory_records = Some(vec![record.clone()]);
            result_record = Some(record);
            cache_updated = true;
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }

        let message = match (params.backend_applied, cache_updated) {
            (true, true) => {
                format!(
                    "Marked memory record {id} as superseded by {superseded_by} and updated Rust cache."
                )
            }
            (true, false) => {
                format!(
                    "Marked memory record {id} as superseded by {superseded_by} in the backend; no matching cached record was present in Rust."
                )
            }
            (false, true) => {
                format!(
                    "Marked memory record {id} as superseded by {superseded_by} in Rust cache; backend mutation was not reported."
                )
            }
            (false, false) => {
                format!(
                    "No cached memory record {id} was present; backend supersede was not reported."
                )
            }
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list all".to_string(),
                description: Some(
                    "Refresh cached Hoppi records including superseded entries after mutation."
                        .to_string(),
                ),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some(
                    "Open the Hoppi dashboard for detailed CRUD actions.".to_string(),
                ),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("action".to_string(), "supersede".to_string());
        metadata.insert("id".to_string(), id.clone());
        metadata.insert("supersededBy".to_string(), superseded_by.clone());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory record superseded".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRecordMutationResult {
            status: self.memory.clone(),
            action: "supersede".to_string(),
            id,
            backend_applied: params.backend_applied,
            cache_updated,
            record: result_record,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn pin_memory_record(
        &mut self,
        params: MemoryRecordPinParams,
    ) -> Result<MemoryRecordMutationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let id = params.id.trim().to_string();
        if id.is_empty() {
            return Err(RuntimeError::new(
                "invalid_memory_record_id",
                RuntimeErrorCategory::InvalidRequest,
                "memory record id cannot be empty",
            ));
        }

        let mut incoming = params.record.clone().map(|mut record| {
            if record.id.trim().is_empty() {
                record.id = id.clone();
            }
            record.pinned = params.pinned;
            record
        });
        if let Some(record) = &incoming
            && record.id != id
        {
            return Err(RuntimeError::new(
                "memory_record_id_mismatch",
                RuntimeErrorCategory::InvalidRequest,
                format!(
                    "memory record id {} does not match requested id {id}",
                    record.id
                ),
            ));
        }

        let mut cache_updated = false;
        let mut result_record = None;
        if let Some(records) = self.memory_records.as_mut() {
            if let Some(index) = records.iter().position(|record| record.id == id) {
                if let Some(record) = incoming.take() {
                    records[index] = record;
                } else {
                    records[index].pinned = params.pinned;
                }
                result_record = Some(records[index].clone());
                cache_updated = true;
            } else if let Some(record) = incoming.take() {
                records.push(record.clone());
                result_record = Some(record);
                cache_updated = true;
            }
        } else if let Some(record) = incoming.take() {
            self.memory_records = Some(vec![record.clone()]);
            result_record = Some(record);
            cache_updated = true;
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        }

        let action = if params.pinned { "pin" } else { "unpin" }.to_string();
        let message = match (params.backend_applied, cache_updated, params.pinned) {
            (true, true, true) => {
                format!("Pinned memory record {id} and updated Rust cache.")
            }
            (true, true, false) => {
                format!("Unpinned memory record {id} and updated Rust cache.")
            }
            (true, false, true) => {
                format!(
                    "Pinned memory record {id} in the backend; no matching cached record was present in Rust."
                )
            }
            (true, false, false) => {
                format!(
                    "Unpinned memory record {id} in the backend; no matching cached record was present in Rust."
                )
            }
            (false, true, true) => {
                format!(
                    "Pinned memory record {id} in Rust cache; backend mutation was not reported."
                )
            }
            (false, true, false) => {
                format!(
                    "Unpinned memory record {id} in Rust cache; backend mutation was not reported."
                )
            }
            (false, false, true) => {
                format!("No cached memory record {id} was present; backend pin was not reported.")
            }
            (false, false, false) => {
                format!("No cached memory record {id} was present; backend unpin was not reported.")
            }
        };
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: Some("Refresh cached Hoppi records after mutation.".to_string()),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some(
                    "Open the Hoppi dashboard for detailed CRUD actions.".to_string(),
                ),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("action".to_string(), action.clone());
        metadata.insert("id".to_string(), id.clone());
        metadata.insert("pinned".to_string(), params.pinned.to_string());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert(
            "source".to_string(),
            params.source.unwrap_or_else(|| "client".to_string()),
        );
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory record pin updated".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryRecordMutationResult {
            status: self.memory.clone(),
            action,
            id,
            backend_applied: params.backend_applied,
            cache_updated,
            record: result_record,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn memory_consolidate(
        &mut self,
        params: MemoryConsolidateParams,
    ) -> Result<MemoryConsolidateResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let stage = params
            .stage
            .as_deref()
            .map(str::trim)
            .filter(|stage| !stage.is_empty())
            .map(str::to_string);
        let summary = params
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|summary| !summary.is_empty())
            .map(str::to_string);
        let model_used = params
            .model_used
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        let source = params
            .source
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty())
            .map(str::to_string);
        let items = params
            .records
            .into_iter()
            .filter_map(|mut record| {
                record.id = record.id.trim().to_string();
                record.content = record.content.trim().to_string();
                (!record.id.is_empty() && !record.content.is_empty()).then_some(record)
            })
            .collect::<Vec<_>>();
        let deleted_ids = params
            .deleted_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();

        let mut cache_updated = false;
        if !params.dry_run {
            let deleted = deleted_ids.iter().cloned().collect::<BTreeSet<_>>();
            if let Some(records) = self.memory_records.as_mut() {
                if !deleted.is_empty() {
                    let before = records.len();
                    records.retain(|record| !deleted.contains(&record.id));
                    cache_updated |= records.len() != before;
                }
                for record in &items {
                    if let Some(existing) = records.iter_mut().find(|cached| cached.id == record.id)
                    {
                        *existing = record.clone();
                    } else {
                        records.push(record.clone());
                    }
                    cache_updated = true;
                }
            } else if !items.is_empty() {
                self.memory_records = Some(items.clone());
                cache_updated = true;
            }
        }

        if let Some(status) = params.status {
            self.memory = status;
        } else if let Some(records) = self.memory_records.as_ref() {
            self.memory.memory_count = records.len() as u64;
            self.memory.pinned_count = records
                .iter()
                .filter(|record| record.pinned || record.starred)
                .count() as u64;
        } else if params.backend_applied {
            self.memory.enabled = true;
            self.memory.initialized = true;
            if self.memory.backend == "none" {
                self.memory.backend = "hoppi".to_string();
            }
        }

        let created_count = items.len() as u64;
        let deleted_count = deleted_ids.len() as u64;
        let total_count = self
            .memory_records
            .as_ref()
            .map(|records| records.len() as u64)
            .unwrap_or(self.memory.memory_count);
        let available = self.memory.enabled
            || params.backend_applied
            || params.dry_run
            || created_count > 0
            || deleted_count > 0
            || summary.is_some();
        let message = match (params.dry_run, params.backend_applied, cache_updated) {
            (true, _, _) => "Previewed Hoppi consolidation result; Rust cache is unchanged.",
            (false, true, true) => "Applied Hoppi consolidation and updated Rust cache.",
            (false, true, false) => {
                "Applied Hoppi consolidation; no cacheable changes were reported."
            }
            (false, false, true) => {
                "Recorded Hoppi consolidation result in Rust cache; backend application was not reported."
            }
            (false, false, false) => {
                "No Hoppi consolidation result was reported; runtime memory state is unchanged."
            }
        }
        .to_string();
        let controls = vec![
            MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list all".to_string(),
                description: Some(
                    "Refresh cached Hoppi records after maintenance or idle consolidation."
                        .to_string(),
                ),
            },
            MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some(
                    "Open the Hoppi dashboard for detailed memory inspection.".to_string(),
                ),
            },
        ];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("dryRun".to_string(), params.dry_run.to_string());
        metadata.insert(
            "backendApplied".to_string(),
            params.backend_applied.to_string(),
        );
        metadata.insert("cacheUpdated".to_string(), cache_updated.to_string());
        metadata.insert("createdCount".to_string(), created_count.to_string());
        metadata.insert("deletedCount".to_string(), deleted_count.to_string());
        metadata.insert("totalCount".to_string(), total_count.to_string());
        metadata.insert(
            "source".to_string(),
            source.clone().unwrap_or_else(|| "client".to_string()),
        );
        if let Some(stage) = &stage {
            metadata.insert("stage".to_string(), stage.clone());
        }
        if let Some(summary) = &summary {
            metadata.insert("summary".to_string(), summary.clone());
        }
        if let Some(model_used) = &model_used {
            metadata.insert("modelUsed".to_string(), model_used.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory consolidation recorded".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryConsolidateResult {
            status: self.memory.clone(),
            available,
            stage,
            summary,
            model_used,
            dry_run: params.dry_run,
            backend_applied: params.backend_applied,
            cache_updated,
            created_count,
            deleted_count,
            total_count,
            items,
            deleted_ids,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn memory_control(
        &mut self,
        params: MemoryControlParams,
    ) -> Result<MemoryControlResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let action = params.action.trim().to_ascii_lowercase();
        let status = self.memory.clone();
        let mut controls = vec![
            MemoryControl {
                id: "dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Show runtime-backed Hoppi status and safe actions.".to_string()),
            },
            MemoryControl {
                id: "enable".to_string(),
                label: "Enable memory".to_string(),
                command: "/memory on".to_string(),
                description: Some(
                    "Use the runtime backend broker when Hoppi is configured.".to_string(),
                ),
            },
            MemoryControl {
                id: "disable".to_string(),
                label: "Disable memory".to_string(),
                command: "/memory off".to_string(),
                description: Some("Disable memory recall/write for native turns.".to_string()),
            },
            MemoryControl {
                id: "maintenance-dry-run".to_string(),
                label: "Preview maintenance".to_string(),
                command: "/memory maintenance dry-run".to_string(),
                description: Some(
                    "Explicitly preview Hoppi maintenance; no hidden idle model session."
                        .to_string(),
                ),
            },
        ];
        let (title, summary) = match action.as_str() {
            "dashboard" | "status" => (
                "Hoppi memory dashboard".to_string(),
                format!(
                    "Memory is {} with backend '{}' in {} scope ({} memories). Rust owns the runtime-owned backend broker; the default backend still uses a bounded Node/TypeScript adapter until pure Rust Hoppi lands.",
                    if status.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    status.backend,
                    status.scope,
                    status.memory_count
                ),
            ),
            "settings" => {
                controls.push(MemoryControl {
                    id: "compact".to_string(),
                    label: "Record compact summary".to_string(),
                    command: "/memory compact <summary>".to_string(),
                    description: Some("Persist an explicit handoff summary event; does not invoke hidden memory workers.".to_string()),
                });
                (
                    "Hoppi memory settings".to_string(),
                    "Native settings are explicit controls over the runtime-owned Hoppi backend broker: enable, disable, compact, and maintenance preview/apply. The default backend still uses a bounded Node/TypeScript adapter until pure Rust Hoppi lands.".to_string(),
                )
            }
            "maintenance" | "maintain" => {
                controls.push(MemoryControl {
                    id: "maintenance-apply".to_string(),
                    label: "Apply maintenance".to_string(),
                    command: "/memory maintenance apply".to_string(),
                    description: Some("Explicit operator action; Rust records intent and never starts hidden idle/deep sessions.".to_string()),
                });
                if params.apply {
                    (
                        "Hoppi memory maintenance".to_string(),
                        "Explicit maintenance apply requested through the runtime-owned Hoppi backend broker; no hidden model session was started.".to_string(),
                    )
                } else {
                    (
                        "Hoppi memory maintenance preview".to_string(),
                        "Dry-run maintenance preview: native Rust reports controls and status only; no memories were changed and no hidden model session was started.".to_string(),
                    )
                }
            }
            other => {
                return Err(RuntimeError::new(
                    "memory_control_unknown_action",
                    RuntimeErrorCategory::InvalidRequest,
                    format!("unknown memory control action: {other}"),
                ));
            }
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("action".to_string(), action);
        metadata.insert("enabled".to_string(), status.enabled.to_string());
        metadata.insert("backend".to_string(), status.backend.clone());
        metadata.insert("scope".to_string(), status.scope.clone());
        metadata.insert("memoryCount".to_string(), status.memory_count.to_string());
        metadata.insert(
            "executionOwner".to_string(),
            status
                .execution_owner
                .clone()
                .unwrap_or_else(|| "runtime".to_string()),
        );
        metadata.insert(
            "executionAdapter".to_string(),
            status
                .execution_adapter
                .clone()
                .unwrap_or_else(|| "configured-runtime-adapter".to_string()),
        );
        metadata.insert("adapterKind".to_string(), "node-typescript".to_string());
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory control action".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryControlResult {
            title,
            summary,
            status,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn memory_dashboard(
        &mut self,
        params: MemoryDashboardParams,
    ) -> Result<MemoryDashboardResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let status = self.memory.clone();
        let url = status
            .dashboard_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let available = url.is_some();
        let fallback_url = url.as_deref().and_then(memory_dashboard_fallback_url);
        let message = if available {
            "Hoppi dashboard is available from runtime memory status.".to_string()
        } else {
            "Hoppi dashboard URL is not available in Rust runtime state; run `/memory dashboard` from a native shell host to start or refresh it.".to_string()
        };
        let controls = vec![MemoryControl {
            id: "open-dashboard".to_string(),
            label: if available {
                "Open dashboard".to_string()
            } else {
                "Start dashboard".to_string()
            },
            command: "/memory dashboard".to_string(),
            description: Some(
                "Use the native host Hoppi dashboard bridge; Rust reports normalized status."
                    .to_string(),
            ),
        }];
        let mut metadata = BTreeMap::new();
        metadata.insert("runtimeOwned".to_string(), "true".to_string());
        metadata.insert("dashboardAvailable".to_string(), available.to_string());
        metadata.insert("backend".to_string(), status.backend.clone());
        metadata.insert("scope".to_string(), status.scope.clone());
        metadata.insert("memoryCount".to_string(), status.memory_count.to_string());
        metadata.insert("pinnedCount".to_string(), status.pinned_count.to_string());
        if let Some(url) = &url {
            metadata.insert("dashboardUrl".to_string(), url.clone());
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory dashboard status".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryDashboardResult {
            status,
            available,
            url,
            fallback_url,
            message,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn todos_state(&self) -> TodoListResult {
        TodoListResult {
            state: self.todos.clone(),
        }
    }

    pub fn apply_todo_client_action(
        &mut self,
        params: TodoClientActionParams,
    ) -> Result<TodoClientActionResult, RuntimeError> {
        self.thread(&params.thread_id)?;

        let mut state = self.todos.clone();
        match params.action {
            TodoClientAction::Clear => {
                state.todos.clear();
                state.summary = "Client cleared todos".to_string();
            }
            TodoClientAction::Done => match params.id {
                Some(id) => {
                    let todo = state
                        .todos
                        .iter_mut()
                        .find(|todo| todo.id == id)
                        .ok_or_else(|| not_found("todo", &id))?;
                    todo.status = TodoStatus::Completed;
                    state.summary = format!("Client marked todo {id} done");
                }
                None => {
                    let mut completed = 0usize;
                    for todo in &mut state.todos {
                        if todo.status != TodoStatus::Completed
                            && todo.status != TodoStatus::Cancelled
                        {
                            todo.status = TodoStatus::Completed;
                            completed += 1;
                        }
                    }
                    state.summary = if completed == 0 {
                        "Client found no active todos to mark done".to_string()
                    } else {
                        format!("Client marked {completed} todo(s) done")
                    };
                }
            },
        }

        self.todos = state.clone();
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::TodosUpdated {
                state: state.clone(),
            },
        );
        Ok(TodoClientActionResult {
            state,
            events: vec![event],
        })
    }

    pub fn compact_handoff(
        &mut self,
        params: HandoffCompactParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let details = params
            .details
            .or_else(|| compaction_details_from_todos(&self.todos));
        let summary = if params.summary.trim().is_empty() {
            deterministic_compaction_summary(details.as_ref())
        } else {
            params.summary
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::HandoffCompacted { summary, details },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_mcp(
        &mut self,
        thread_id: &str,
        mut server: McpServerRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        if server.source.is_none() {
            server.source = Some(McpServerSource::Runtime);
        }
        self.mcp_servers.insert(server.id.clone(), server.clone());
        let event = self.event(thread_id, None, EventKind::McpServerRegistered { server });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    #[cfg(test)]
    pub fn register_mock_mcp_server(&mut self, server_id: &str, responses: Vec<Value>) {
        self.mcp_mock_servers.insert(
            server_id.to_string(),
            responses.into_iter().collect::<VecDeque<_>>(),
        );
        self.mcp_servers.insert(
            server_id.to_string(),
            McpServerRef {
                id: server_id.to_string(),
                name: server_id.to_string(),
                status: McpServerStatus::Enabled,
                source: Some(McpServerSource::Runtime),
                transport: None,
                trusted: true,
                description: Some("Mock MCP server".to_string()),
                tools: Vec::new(),
                tool_refs: Vec::new(),
                resource_subscriptions: Vec::new(),
                when_to_use: None,
                capabilities: McpServerCapabilities::default(),
                list_refresh_hints: McpListRefreshHints::default(),
                diagnostics: Vec::new(),
            },
        );
    }

    #[cfg(test)]
    pub fn register_mock_plugin_runner(&mut self, plugin_id: &str, responses: Vec<Value>) {
        self.plugin_runner_mock_runners.insert(
            plugin_id.to_string(),
            responses.into_iter().collect::<VecDeque<_>>(),
        );
    }

    fn plugin_runner_for_thread(
        &self,
        thread_id: &str,
        plugin_id: &str,
    ) -> Result<ConfiguredPluginRunner, RuntimeError> {
        let cwd = self.thread(thread_id)?.project.cwd.clone();
        configured_plugin_runner_entries(&cwd)
            .into_iter()
            .find(|runner| runner.plugin_id == plugin_id)
            .ok_or_else(|| not_found("plugin runner", plugin_id))
    }

    fn plugin_runner_transport_for_runner(
        &mut self,
        runner: &ConfiguredPluginRunner,
    ) -> Result<Box<dyn McpTransportSession>, RuntimeError> {
        #[cfg(test)]
        if let Some(responses) = self.plugin_runner_mock_runners.remove(&runner.plugin_id) {
            return Ok(Box::new(MockMcpTransport::new(responses)));
        }

        let transport = McpTransportConfig::Stdio {
            command: runner.command.clone(),
            args: runner.args.clone(),
            env: BTreeMap::new(),
            cwd: Some(runner.root.to_string_lossy().to_string()),
        };
        StdioMcpTransport::spawn(&transport).map(|transport| Box::new(transport) as _)
    }

    fn discover_plugin_runner_tools(
        &mut self,
        runner: &ConfiguredPluginRunner,
    ) -> Result<Vec<PluginRunnerToolRef>, RuntimeError> {
        if !runner
            .capabilities
            .iter()
            .any(|capability| capability == "tools")
        {
            return Ok(Vec::new());
        }
        let mut transport = self.plugin_runner_transport_for_runner(runner)?;
        let result = (|| -> Result<Vec<PluginRunnerToolRef>, RuntimeError> {
            let _initialize = transport.request(
                "initialize",
                json!({
                    "client": {
                        "name": "OPPi",
                        "version": "native"
                    },
                    "pluginId": runner.plugin_id,
                }),
            )?;
            transport.notify("notifications/initialized", json!({}))?;
            let tools = transport.request("tools/list", json!({}))?;
            plugin_runner_tool_refs_from_tools_list(&runner.plugin_id, &tools)
        })();
        transport.shutdown();
        result
    }

    pub fn plugin_runner_tool_definitions_for_thread(
        &mut self,
        thread_id: &str,
    ) -> Result<Vec<ToolDefinition>, RuntimeError> {
        let cwd = self.thread(thread_id)?.project.cwd.clone();
        let runners = configured_plugin_runner_entries(&cwd);
        let enabled_ids = runners
            .iter()
            .map(|runner| runner.plugin_id.clone())
            .collect::<BTreeSet<_>>();
        self.plugin_runner_tools
            .retain(|plugin_id, _| enabled_ids.contains(plugin_id));

        let mut definitions = Vec::new();
        for runner in runners {
            let tools = self.discover_plugin_runner_tools(&runner)?;
            definitions.extend(tools.iter().map(plugin_runner_tool_definition));
            self.plugin_runner_tools
                .insert(runner.plugin_id.clone(), tools);
        }
        Ok(definitions)
    }

    pub fn list_mcp(
        &self,
        params: McpListParams,
    ) -> Result<RuntimeListResult<McpServerRef>, RuntimeError> {
        let cwd = params
            .thread_id
            .as_deref()
            .map(|thread_id| {
                self.thread(thread_id)
                    .map(|thread| thread.project.cwd.clone())
            })
            .transpose()?;
        let mut items = self.mcp_servers.clone();
        if let Some(cwd) = cwd.as_deref() {
            for server in configured_mcp_refs(Some(cwd)) {
                items.entry(server.id.clone()).or_insert(server);
            }
        }
        Ok(RuntimeListResult {
            items: items.into_values().collect(),
        })
    }

    pub fn trust_plugin_mcp(
        &mut self,
        params: PluginMcpTrustParams,
    ) -> Result<RuntimeListResult<McpServerRef>, RuntimeError> {
        let cwd = self.thread(&params.thread_id)?.project.cwd.clone();
        let exists = configured_plugin_mcp_roots(Some(&cwd))
            .into_iter()
            .filter(|entry| entry.plugin_id == params.plugin_id)
            .any(|entry| {
                plugin_mcp_refs_from_root(&entry.root, &entry.plugin_id, &[])
                    .into_iter()
                    .any(|server| server.id == params.server_id)
            });
        if !exists {
            return Err(not_found(
                "plugin mcp server",
                &format!("{}/{}", params.plugin_id, params.server_id),
            ));
        }

        let mut store = read_plugin_mcp_trust_store(&cwd);
        store.servers.retain(|record| {
            !(record.plugin_id == params.plugin_id && record.server_id == params.server_id)
        });
        store.servers.push(StoredPluginMcpTrustRecord {
            plugin_id: params.plugin_id,
            server_id: params.server_id,
            enabled: params.enabled,
            trusted: params.trusted,
            updated_at: now_millis().to_string(),
        });
        store.servers.sort_by(|left, right| {
            left.plugin_id
                .cmp(&right.plugin_id)
                .then_with(|| left.server_id.cmp(&right.server_id))
        });
        write_plugin_mcp_trust_store(&cwd, &store)?;

        self.list_mcp(McpListParams {
            thread_id: Some(params.thread_id),
        })
    }

    fn mcp_server_for_thread(
        &self,
        thread_id: &str,
        server_id: &str,
    ) -> Result<McpServerRef, RuntimeError> {
        let thread = self.thread(thread_id)?;
        if let Some(server) = self.mcp_servers.get(server_id) {
            return Ok(server.clone());
        }
        configured_mcp_refs(Some(&thread.project.cwd))
            .into_iter()
            .find(|server| server.id == server_id)
            .ok_or_else(|| not_found("mcp server", server_id))
    }

    fn mcp_transport_for_server(
        &mut self,
        server: &McpServerRef,
    ) -> Result<Box<dyn McpTransportSession>, RuntimeError> {
        #[cfg(test)]
        if let Some(responses) = self.mcp_mock_servers.remove(&server.id) {
            return Ok(Box::new(MockMcpTransport::new(responses)));
        }

        if !server.trusted || server.status == McpServerStatus::Disabled {
            return Err(mcp_error(
                "mcp_server_untrusted",
                format!(
                    "MCP server {} is disabled or untrusted; enable and trust it before connecting",
                    server.id
                ),
            ));
        }
        let Some(transport) = server.transport.as_ref() else {
            return Err(mcp_error(
                "mcp_transport_missing",
                format!("MCP server {} has no transport configuration", server.id),
            ));
        };
        match transport {
            McpTransportConfig::Stdio { .. } => Ok(Box::new(StdioMcpTransport::spawn(transport)?)),
            McpTransportConfig::StreamableHttp { endpoint, auth_ref } => {
                Ok(Box::new(StreamableHttpMcpTransport::new(
                    endpoint.clone(),
                    auth_ref.as_deref(),
                    self.mcp_http_sessions
                        .get(&server.id)
                        .cloned()
                        .unwrap_or_default(),
                )?))
            }
        }
    }

    fn ingest_mcp_transport_notifications(
        &mut self,
        thread_id: &str,
        server_id: &str,
        notifications: Vec<Value>,
    ) {
        for notification in notifications {
            if let Some(params) =
                mcp_resource_update_from_transport_notification(thread_id, server_id, &notification)
            {
                let _ = self.mcp_resource_updated(params);
            } else if let Some(kind) =
                mcp_list_changed_kind_from_transport_notification(&notification)
            {
                self.record_mcp_list_changed(thread_id, server_id, kind);
            }
        }
    }

    fn record_mcp_list_changed(
        &mut self,
        thread_id: &str,
        server_id: &str,
        kind: McpListChangedKind,
    ) {
        let list_kind = kind.label();
        let code = kind.code();
        let message = format!("MCP {list_kind} list changed");
        if let Some(mut server) = self.mcp_servers.get(server_id).cloned() {
            server
                .diagnostics
                .retain(|diagnostic| diagnostic.code != code);
            server.diagnostics.push(mcp_diagnostic(
                code.to_string(),
                message.clone(),
                Some("info".to_string()),
            ));
            let hint = kind
                .refresh_hint_slot(&mut server.list_refresh_hints)
                .get_or_insert_with(McpListRefreshHint::default);
            hint.refresh_count = hint.refresh_count.saturating_add(1);
            self.mcp_servers.insert(server.id.clone(), server);
        }
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message,
            metadata: BTreeMap::from([
                ("component".to_string(), "mcp".to_string()),
                ("code".to_string(), code.to_string()),
                ("serverId".to_string(), server_id.to_string()),
                ("listKind".to_string(), list_kind.to_string()),
            ]),
        };
        self.event(thread_id, None, EventKind::Diagnostic { diagnostic });
    }

    fn clear_mcp_list_refresh_hint(&mut self, server_id: &str, kind: McpListChangedKind) {
        if let Some(server) = self.mcp_servers.get_mut(server_id) {
            *kind.refresh_hint_slot(&mut server.list_refresh_hints) = None;
            let code = kind.code();
            server
                .diagnostics
                .retain(|diagnostic| diagnostic.code != code);
        }
    }

    fn failed_mcp_connect_result(
        &mut self,
        thread_id: &str,
        server_id: &str,
        mut server: McpServerRef,
        error: RuntimeError,
    ) -> EventsListResult {
        let code = error.code;
        let message = error.message;
        server.status = McpServerStatus::Failed;
        server.diagnostics.push(mcp_diagnostic(
            code.clone(),
            message.clone(),
            Some("error".to_string()),
        ));
        self.mcp_servers.insert(server.id.clone(), server.clone());
        let registered = self.event(thread_id, None, EventKind::McpServerRegistered { server });
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Error,
            message,
            metadata: BTreeMap::from([
                ("component".to_string(), "mcp".to_string()),
                ("code".to_string(), code),
                ("serverId".to_string(), server_id.to_string()),
            ]),
        };
        let diagnostic = self.event(thread_id, None, EventKind::Diagnostic { diagnostic });
        EventsListResult {
            events: vec![registered, diagnostic],
        }
    }

    pub fn connect_mcp(
        &mut self,
        params: McpConnectParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let mut server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "mcp_stdio_spawn_failed"
                        | "mcp_stdio_stdin_unavailable"
                        | "mcp_stdio_stdout_unavailable"
                ) =>
            {
                return Ok(self.failed_mcp_connect_result(
                    &params.thread_id,
                    &params.server_id,
                    server,
                    error,
                ));
            }
            Err(error) => return Err(error),
        };
        let connect_result = (|| -> Result<(Value, Vec<McpToolRef>), RuntimeError> {
            let initialize = transport.request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "clientInfo": {
                        "name": "oppi-native",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {
                        "tools": {}
                    }
                }),
            )?;
            transport.notify("notifications/initialized", json!({}))?;
            let tools_result = transport.request("tools/list", json!({}))?;
            let tool_refs = mcp_tool_refs_from_tools_list(&server.id, &tools_result)?;
            Ok((initialize, tool_refs))
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();

        let (initialize, tool_refs) = match connect_result {
            Ok(result) => {
                if let Some(session_state) = session_state {
                    self.mcp_http_sessions
                        .insert(server.id.clone(), session_state);
                }
                result
            }
            Err(error) => {
                return Ok(self.failed_mcp_connect_result(
                    &params.thread_id,
                    &params.server_id,
                    server,
                    error,
                ));
            }
        };
        let tools = tool_refs
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        if let Some(name) = initialize
            .get("serverInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            server.name = name.to_string();
        }
        server.status = McpServerStatus::Enabled;
        server.trusted = true;
        server.tools = tools;
        server.tool_refs = tool_refs;
        server.capabilities = mcp_server_capabilities_from_initialize(&initialize);
        if !server
            .capabilities
            .resources
            .as_ref()
            .and_then(|resources| resources.subscribe)
            .unwrap_or(false)
        {
            server.resource_subscriptions.clear();
        }
        server.diagnostics.retain(|diagnostic| {
            diagnostic.code != "mcp_requires_trust"
                && diagnostic.code != "mcp_plugin_metadata_only"
                && diagnostic.code != "mcp_transport_untrusted"
        });
        if let Some(version) = initialize.get("protocolVersion").and_then(Value::as_str) {
            server.diagnostics.push(mcp_diagnostic(
                "mcp_connected",
                format!("MCP server initialized with protocol {version}."),
                Some("info".to_string()),
            ));
        }
        sync_mcp_notification_listener_diagnostic(&mut server);
        self.mcp_servers.insert(server.id.clone(), server.clone());
        self.clear_mcp_list_refresh_hint(&server.id, McpListChangedKind::Tools);
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        let server = self.mcp_servers.get(&server.id).cloned().unwrap_or(server);
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::McpServerRegistered { server },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn mcp_action(
        &mut self,
        params: McpActionParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        let can_connect = server.transport.is_some() || {
            #[cfg(test)]
            {
                self.mcp_mock_servers.contains_key(&params.server_id)
            }
            #[cfg(not(test))]
            {
                false
            }
        };
        if matches!(params.action, McpAction::Reload | McpAction::Test) && can_connect {
            return self.connect_mcp(McpConnectParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
            });
        }
        if params.action == McpAction::Auth {
            let mut server = server;
            server.status = McpServerStatus::AuthRequired;
            let guidance = mcp_auth_guidance_for_server(&server);
            server.diagnostics.push(mcp_diagnostic(
                "mcp_auth_required",
                guidance.clone(),
                Some("warning".to_string()),
            ));
            self.mcp_servers.insert(server.id.clone(), server.clone());
            let registered = self.event(
                &params.thread_id,
                None,
                EventKind::McpServerRegistered {
                    server: server.clone(),
                },
            );
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Warning,
                message: format!("MCP auth required for {}: {guidance}", params.server_id),
                metadata: {
                    let mut metadata = BTreeMap::from([
                        ("serverId".to_string(), params.server_id),
                        ("component".to_string(), "mcp".to_string()),
                        ("code".to_string(), "mcp_auth_required".to_string()),
                    ]);
                    if let Some((env_name, state)) = mcp_auth_ref_env_state_for_server(&server) {
                        metadata.insert("authRefEnv".to_string(), env_name);
                        metadata.insert("authRefState".to_string(), state);
                    }
                    metadata
                },
            };
            let diagnostic = self.event(
                &params.thread_id,
                None,
                EventKind::Diagnostic { diagnostic },
            );
            return Ok(EventsListResult {
                events: vec![registered, diagnostic],
            });
        }
        let action = match params.action {
            McpAction::Reload => "reload",
            McpAction::Test => "test",
            McpAction::Auth => "auth",
        };
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: format!("MCP {action} flow recorded for {}", params.server_id),
            metadata: BTreeMap::from([
                ("serverId".to_string(), params.server_id),
                ("component".to_string(), "mcp".to_string()),
            ]),
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic { diagnostic },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn call_mcp_tool(
        &mut self,
        params: McpCallToolParams,
    ) -> Result<McpCallToolResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpCallToolResult {
                server_id: params.server_id,
                tool_name: params.tool_name,
                status: ToolResultStatus::Denied,
                content: Vec::new(),
                structured_content: None,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let Some(tool) = server
            .tool_refs
            .iter()
            .find(|tool| tool.name == params.tool_name)
            .cloned()
        else {
            return Ok(McpCallToolResult {
                server_id: params.server_id,
                tool_name: params.tool_name,
                status: ToolResultStatus::Error,
                content: Vec::new(),
                structured_content: None,
                error: Some("MCP tool is not known for this connected server".to_string()),
            });
        };
        if let Err(error) = validate_mcp_tool_arguments(Some(&tool.input_schema), &params.arguments)
        {
            return Ok(McpCallToolResult {
                server_id: params.server_id,
                tool_name: params.tool_name,
                status: ToolResultStatus::Error,
                content: Vec::new(),
                structured_content: None,
                error: Some(error),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpCallToolResult {
                    server_id: params.server_id,
                    tool_name: params.tool_name,
                    status: ToolResultStatus::Error,
                    content: Vec::new(),
                    structured_content: None,
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = mcp_transport_should_initialize(&server, &self.mcp_http_sessions);
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                initialize_mcp_transport(&mut *transport, "tools")?;
            }
            transport.request(
                "tools/call",
                json!({
                    "name": tool.name,
                    "arguments": params.arguments,
                }),
            )
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match result {
            Ok(result) => Ok(mcp_call_tool_result_from_value(
                params.server_id,
                params.tool_name,
                result,
            )),
            Err(error) => Ok(McpCallToolResult {
                server_id: params.server_id,
                tool_name: params.tool_name,
                status: ToolResultStatus::Error,
                content: Vec::new(),
                structured_content: None,
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    pub fn list_mcp_resources(
        &mut self,
        params: McpListResourcesParams,
    ) -> Result<McpListResourcesResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpListResourcesResult {
                server_id: params.server_id,
                status: ToolResultStatus::Denied,
                resources: Vec::new(),
                next_cursor: None,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpListResourcesResult {
                    server_id: params.server_id,
                    status: ToolResultStatus::Error,
                    resources: Vec::new(),
                    next_cursor: None,
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = match server.transport.as_ref() {
            Some(McpTransportConfig::StreamableHttp { .. }) => {
                !self.mcp_http_sessions.contains_key(&server.id)
            }
            _ => true,
        };
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                transport.request(
                    "initialize",
                    json!({
                        "protocolVersion": "2025-11-25",
                        "clientInfo": {
                            "name": "oppi-native",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "resources": {}
                        }
                    }),
                )?;
                transport.notify("notifications/initialized", json!({}))?;
            }
            let params = params
                .cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            transport.request("resources/list", params)
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        let parsed =
            result.and_then(|value| mcp_resource_refs_from_resources_list(&server.id, &value));
        if parsed.is_ok() {
            self.clear_mcp_list_refresh_hint(&server.id, McpListChangedKind::Resources);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match parsed {
            Ok((resources, next_cursor)) => Ok(McpListResourcesResult {
                server_id: server.id,
                status: ToolResultStatus::Ok,
                resources,
                next_cursor,
                error: None,
            }),
            Err(error) => Ok(McpListResourcesResult {
                server_id: server.id,
                status: ToolResultStatus::Error,
                resources: Vec::new(),
                next_cursor: None,
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    pub fn list_mcp_resource_templates(
        &mut self,
        params: McpListResourceTemplatesParams,
    ) -> Result<McpListResourceTemplatesResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpListResourceTemplatesResult {
                server_id: params.server_id,
                status: ToolResultStatus::Denied,
                resource_templates: Vec::new(),
                next_cursor: None,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpListResourceTemplatesResult {
                    server_id: params.server_id,
                    status: ToolResultStatus::Error,
                    resource_templates: Vec::new(),
                    next_cursor: None,
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = match server.transport.as_ref() {
            Some(McpTransportConfig::StreamableHttp { .. }) => {
                !self.mcp_http_sessions.contains_key(&server.id)
            }
            _ => true,
        };
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                transport.request(
                    "initialize",
                    json!({
                        "protocolVersion": "2025-11-25",
                        "clientInfo": {
                            "name": "oppi-native",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "resources": {}
                        }
                    }),
                )?;
                transport.notify("notifications/initialized", json!({}))?;
            }
            let params = params
                .cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            transport.request("resources/templates/list", params)
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match result
            .and_then(|value| mcp_resource_template_refs_from_templates_list(&server.id, &value))
        {
            Ok((resource_templates, next_cursor)) => Ok(McpListResourceTemplatesResult {
                server_id: server.id,
                status: ToolResultStatus::Ok,
                resource_templates,
                next_cursor,
                error: None,
            }),
            Err(error) => Ok(McpListResourceTemplatesResult {
                server_id: server.id,
                status: ToolResultStatus::Error,
                resource_templates: Vec::new(),
                next_cursor: None,
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    pub fn list_mcp_prompts(
        &mut self,
        params: McpListPromptsParams,
    ) -> Result<McpListPromptsResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpListPromptsResult {
                server_id: params.server_id,
                status: ToolResultStatus::Denied,
                prompts: Vec::new(),
                next_cursor: None,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpListPromptsResult {
                    server_id: params.server_id,
                    status: ToolResultStatus::Error,
                    prompts: Vec::new(),
                    next_cursor: None,
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = match server.transport.as_ref() {
            Some(McpTransportConfig::StreamableHttp { .. }) => {
                !self.mcp_http_sessions.contains_key(&server.id)
            }
            _ => true,
        };
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                transport.request(
                    "initialize",
                    json!({
                        "protocolVersion": "2025-11-25",
                        "clientInfo": {
                            "name": "oppi-native",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "prompts": {}
                        }
                    }),
                )?;
                transport.notify("notifications/initialized", json!({}))?;
            }
            let params = params
                .cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            transport.request("prompts/list", params)
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        let parsed = result.and_then(|value| mcp_prompt_refs_from_prompts_list(&server.id, &value));
        if parsed.is_ok() {
            self.clear_mcp_list_refresh_hint(&server.id, McpListChangedKind::Prompts);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match parsed {
            Ok((prompts, next_cursor)) => Ok(McpListPromptsResult {
                server_id: server.id,
                status: ToolResultStatus::Ok,
                prompts,
                next_cursor,
                error: None,
            }),
            Err(error) => Ok(McpListPromptsResult {
                server_id: server.id,
                status: ToolResultStatus::Error,
                prompts: Vec::new(),
                next_cursor: None,
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    pub fn get_mcp_prompt(
        &mut self,
        params: McpGetPromptParams,
    ) -> Result<McpGetPromptResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpGetPromptResult {
                server_id: params.server_id,
                name: params.name,
                status: ToolResultStatus::Denied,
                description: None,
                messages: Vec::new(),
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpGetPromptResult {
                    server_id: params.server_id,
                    name: params.name,
                    status: ToolResultStatus::Error,
                    description: None,
                    messages: Vec::new(),
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = match server.transport.as_ref() {
            Some(McpTransportConfig::StreamableHttp { .. }) => {
                !self.mcp_http_sessions.contains_key(&server.id)
            }
            _ => true,
        };
        let request_name = params.name.clone();
        let request_arguments = params.arguments.clone();
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                transport.request(
                    "initialize",
                    json!({
                        "protocolVersion": "2025-11-25",
                        "clientInfo": {
                            "name": "oppi-native",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "prompts": {}
                        }
                    }),
                )?;
                transport.notify("notifications/initialized", json!({}))?;
            }
            let mut payload = json!({ "name": request_name });
            if !request_arguments.is_empty() {
                payload["arguments"] = json!(request_arguments);
            }
            transport.request("prompts/get", payload)
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match result.and_then(|value| mcp_prompt_messages_from_get_result(&value)) {
            Ok((description, messages)) => Ok(McpGetPromptResult {
                server_id: server.id,
                name: params.name,
                status: ToolResultStatus::Ok,
                description,
                messages,
                error: None,
            }),
            Err(error) => Ok(McpGetPromptResult {
                server_id: server.id,
                name: params.name,
                status: ToolResultStatus::Error,
                description: None,
                messages: Vec::new(),
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    pub fn read_mcp_resource(
        &mut self,
        params: McpReadResourceParams,
    ) -> Result<McpReadResourceResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpReadResourceResult {
                server_id: params.server_id,
                uri: params.uri,
                status: ToolResultStatus::Denied,
                contents: Vec::new(),
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpReadResourceResult {
                    server_id: params.server_id,
                    uri: params.uri,
                    status: ToolResultStatus::Error,
                    contents: Vec::new(),
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let should_initialize = match server.transport.as_ref() {
            Some(McpTransportConfig::StreamableHttp { .. }) => {
                !self.mcp_http_sessions.contains_key(&server.id)
            }
            _ => true,
        };
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                transport.request(
                    "initialize",
                    json!({
                        "protocolVersion": "2025-11-25",
                        "clientInfo": {
                            "name": "oppi-native",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "resources": {}
                        }
                    }),
                )?;
                transport.notify("notifications/initialized", json!({}))?;
            }
            transport.request("resources/read", json!({ "uri": params.uri }))
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        self.ingest_mcp_transport_notifications(&params.thread_id, &server.id, notifications);
        match result.and_then(|value| mcp_resource_contents_from_read_result(&value)) {
            Ok(contents) => Ok(McpReadResourceResult {
                server_id: server.id,
                uri: params.uri,
                status: ToolResultStatus::Ok,
                contents,
                error: None,
            }),
            Err(error) => Ok(McpReadResourceResult {
                server_id: server.id,
                uri: params.uri,
                status: ToolResultStatus::Error,
                contents: Vec::new(),
                error: Some(format!("{}: {}", error.code, error.message)),
            }),
        }
    }

    fn change_mcp_resource_subscription(
        &mut self,
        params: McpResourceSubscriptionParams,
        subscribe: bool,
    ) -> Result<McpResourceSubscriptionResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpResourceSubscriptionResult {
                server_id: params.server_id,
                uri: params.uri,
                status: ToolResultStatus::Denied,
                subscribed: false,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }
        let supports_resource_subscriptions = server
            .capabilities
            .resources
            .as_ref()
            .and_then(|resources| resources.subscribe)
            .unwrap_or(false);
        if !supports_resource_subscriptions {
            return Ok(McpResourceSubscriptionResult {
                server_id: params.server_id,
                uri: params.uri,
                status: ToolResultStatus::Denied,
                subscribed: false,
                error: Some("MCP server does not advertise resources.subscribe".to_string()),
            });
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                return Ok(McpResourceSubscriptionResult {
                    server_id: params.server_id,
                    uri: params.uri,
                    status: ToolResultStatus::Error,
                    subscribed: false,
                    error: Some(format!("{}: {}", error.code, error.message)),
                });
            }
        };
        let request_uri = params.uri.clone();
        let method = if subscribe {
            "resources/subscribe"
        } else {
            "resources/unsubscribe"
        };
        let should_initialize = mcp_transport_should_initialize(&server, &self.mcp_http_sessions);
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                initialize_mcp_transport(&mut *transport, "resources")?;
            }
            transport.request(method, json!({ "uri": request_uri }))
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(server.id.clone(), session_state);
        }
        match result {
            Ok(result)
                if result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                self.ingest_mcp_transport_notifications(
                    &params.thread_id,
                    &server.id,
                    notifications,
                );
                Ok(McpResourceSubscriptionResult {
                    server_id: server.id,
                    uri: params.uri,
                    status: ToolResultStatus::Error,
                    subscribed: false,
                    error: Some(mcp_call_result_output(&result)),
                })
            }
            Ok(_) => {
                if let Some(server) = self.mcp_servers.get_mut(&server.id) {
                    if subscribe {
                        if !server
                            .resource_subscriptions
                            .iter()
                            .any(|subscription| subscription.uri == params.uri)
                        {
                            server
                                .resource_subscriptions
                                .push(McpResourceSubscriptionRef {
                                    uri: params.uri.clone(),
                                    update_count: 0,
                                    last_updated_at: None,
                                });
                        }
                    } else {
                        server
                            .resource_subscriptions
                            .retain(|subscription| subscription.uri != params.uri);
                    }
                    server
                        .resource_subscriptions
                        .sort_by(|left, right| left.uri.cmp(&right.uri));
                }
                self.ingest_mcp_transport_notifications(
                    &params.thread_id,
                    &server.id,
                    notifications,
                );
                Ok(McpResourceSubscriptionResult {
                    server_id: server.id,
                    uri: params.uri,
                    status: ToolResultStatus::Ok,
                    subscribed: subscribe,
                    error: None,
                })
            }
            Err(error) => {
                self.ingest_mcp_transport_notifications(
                    &params.thread_id,
                    &server.id,
                    notifications,
                );
                Ok(McpResourceSubscriptionResult {
                    server_id: server.id,
                    uri: params.uri,
                    status: ToolResultStatus::Error,
                    subscribed: false,
                    error: Some(format!("{}: {}", error.code, error.message)),
                })
            }
        }
    }

    pub fn subscribe_mcp_resource(
        &mut self,
        params: McpResourceSubscriptionParams,
    ) -> Result<McpResourceSubscriptionResult, RuntimeError> {
        self.change_mcp_resource_subscription(params, true)
    }

    pub fn unsubscribe_mcp_resource(
        &mut self,
        params: McpResourceSubscriptionParams,
    ) -> Result<McpResourceSubscriptionResult, RuntimeError> {
        self.change_mcp_resource_subscription(params, false)
    }

    pub fn mcp_resource_updated(
        &mut self,
        params: McpResourceUpdatedParams,
    ) -> Result<McpResourceUpdatedResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let server = self.mcp_server_for_thread(&params.thread_id, &params.server_id)?;
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return Ok(McpResourceUpdatedResult {
                server_id: params.server_id,
                uri: params.uri,
                status: ToolResultStatus::Denied,
                updated: false,
                update_count: 0,
                error: Some("MCP server is not connected and trusted".to_string()),
            });
        }

        let mut update_count = 0;
        let mut updated = false;
        if let Some(server) = self.mcp_servers.get_mut(&server.id)
            && let Some(subscription) = server
                .resource_subscriptions
                .iter_mut()
                .find(|subscription| subscription.uri == params.uri)
        {
            subscription.update_count = subscription.update_count.saturating_add(1);
            if params.received_at.is_some() {
                subscription.last_updated_at = params.received_at.clone();
            }
            update_count = subscription.update_count;
            updated = true;
        }

        if !updated {
            return Ok(McpResourceUpdatedResult {
                server_id: server.id,
                uri: params.uri,
                status: ToolResultStatus::Denied,
                updated: false,
                update_count: 0,
                error: Some(
                    "MCP resource update ignored because the URI is not actively subscribed"
                        .to_string(),
                ),
            });
        }

        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "MCP subscribed resource updated".to_string(),
            metadata: BTreeMap::from([
                ("component".to_string(), "mcp".to_string()),
                ("serverId".to_string(), server.id.clone()),
                ("updateCount".to_string(), update_count.to_string()),
            ]),
        };
        self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic { diagnostic },
        );

        Ok(McpResourceUpdatedResult {
            server_id: server.id,
            uri: params.uri,
            status: ToolResultStatus::Ok,
            updated: true,
            update_count,
            error: None,
        })
    }

    fn mcp_subscribed_resource_context(
        &mut self,
        thread_id: &str,
    ) -> Result<Option<McpSubscribedResourceContext>, RuntimeError> {
        let servers = self
            .list_mcp(McpListParams {
                thread_id: Some(thread_id.to_string()),
            })?
            .items;
        let mut resources = 0usize;
        let mut skipped = 0usize;
        let mut sections = Vec::new();
        'servers: for server in servers {
            if server.status != McpServerStatus::Enabled
                || !server.trusted
                || server.resource_subscriptions.is_empty()
            {
                continue;
            }
            for subscription in server.resource_subscriptions {
                if resources >= MAX_MCP_SUBSCRIBED_RESOURCES {
                    skipped = skipped.saturating_add(1);
                    continue 'servers;
                }
                let result = self.read_mcp_resource(McpReadResourceParams {
                    thread_id: thread_id.to_string(),
                    server_id: server.id.clone(),
                    uri: subscription.uri.clone(),
                })?;
                if result.status != ToolResultStatus::Ok {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                let mut content_lines = Vec::new();
                for content in result.contents {
                    if let Some(text) = content
                        .text
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        content_lines.push(truncate_chars(text, MAX_MCP_SUBSCRIBED_RESOURCE_CHARS));
                    } else if let Some(blob) = content.blob.as_deref() {
                        let mime = content
                            .mime_type
                            .as_deref()
                            .unwrap_or("application/octet-stream");
                        content_lines.push(format!(
                            "[blob resource: mimeType={mime} base64Chars={}]",
                            blob.len()
                        ));
                    }
                }
                if content_lines.is_empty() {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                sections.push(format!(
                    "Resource: {} {}\n{}",
                    server.id,
                    result.uri,
                    content_lines.join("\n")
                ));
                resources = resources.saturating_add(1);
            }
        }
        if sections.is_empty() {
            return Ok(None);
        }
        Ok(Some(McpSubscribedResourceContext {
            prompt: truncate_chars(&sections.join("\n\n"), MAX_MCP_SUBSCRIBED_CONTEXT_CHARS),
            resources,
            skipped,
        }))
    }

    pub fn register_model(
        &mut self,
        thread_id: &str,
        model: ModelRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        self.models.insert(model.id.clone(), model.clone());
        let event = self.event(thread_id, None, EventKind::ModelRegistered { model });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_models(&self) -> RuntimeListResult<ModelRef> {
        RuntimeListResult {
            items: self.models.values().cloned().collect(),
        }
    }

    pub fn list_model_registry(&self, params: ModelRegistryListParams) -> ModelRegistryListResult {
        let provider = params.provider.trim();
        let provider = if provider.is_empty() {
            OPENAI_CODEX_PROVIDER_ID
        } else {
            provider
        }
        .to_string();
        let selected_model_id = params
            .selected_model_id
            .as_deref()
            .or(self.selected_model.as_deref());
        let mut entries = model_registry_builtin_model_ids(&provider)
            .iter()
            .map(|model| ModelRef {
                id: (*model).to_string(),
                provider: provider.clone(),
                display_name: (*model).to_string(),
                context_window: None,
                role: None,
            })
            .collect::<Vec<_>>();
        for model in self.models.values() {
            if model.provider == provider {
                model_registry_upsert(&mut entries, model.clone());
            }
        }

        let models_path = model_registry_models_json_path();
        let (providers, mut diagnostics) = model_registry_providers_from_path(&models_path);
        if let Some(providers) = providers.as_ref() {
            diagnostics.extend(model_registry_apply_models_json(
                &provider,
                &models_path,
                providers,
                &mut entries,
            ));
        }
        if params.available_only
            && !model_registry_builtin_auth_configured(&provider, providers.as_ref())
        {
            entries.clear();
        }
        let items =
            model_registry_order_models(&provider, &entries, selected_model_id, &params.scope);
        let selected_model = selected_model_id.and_then(|selected| {
            items
                .iter()
                .find(|model| model.id == selected)
                .cloned()
                .or_else(|| entries.iter().find(|model| model.id == selected).cloned())
        });

        ModelRegistryListResult {
            provider,
            scope: params.scope,
            available_only: params.available_only,
            selected_model,
            items,
            diagnostics,
        }
    }

    pub fn select_model(
        &mut self,
        params: ModelSelectParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        if !self.models.contains_key(&params.model_id) {
            return Err(not_found("model", &params.model_id));
        }
        self.selected_model = Some(params.model_id.clone());
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::ModelSelected {
                model_id: params.model_id,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_agent(
        &mut self,
        thread_id: &str,
        agent: AgentDefinition,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let definitions = self.agents.entry(agent.name.clone()).or_default();
        definitions.push(agent.clone());
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: format!("registered agent {}", agent.name),
            metadata: BTreeMap::from([("description".to_string(), agent.description)]),
        };
        let event = self.event(thread_id, None, EventKind::Diagnostic { diagnostic });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_agents(&self, params: AgentListParams) -> Result<AgentListResult, RuntimeError> {
        let project_root = self.agent_project_root(params.thread_id.as_deref())?;
        let home_dir = user_home_base_dir().unwrap_or_else(|| PathBuf::from("."));
        let definitions = self
            .agents
            .values()
            .flat_map(|definitions| definitions.iter().cloned())
            .collect();
        let index = load_agent_index(AgentIndexOptions {
            project_root,
            home_dir,
            agent_dir: default_oppi_agent_dir(),
            built_ins: definitions,
        })
        .map_err(|error| {
            RuntimeError::new(
                "agent_index_load_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!("failed to load agent definitions: {error}"),
            )
        })?;
        Ok(AgentListResult {
            items: index.items,
            failed: index.failed,
        })
    }

    pub fn show_agent(&self, params: AgentShowParams) -> Result<ResolvedAgent, RuntimeError> {
        self.list_agents(AgentListParams {
            thread_id: Some(params.thread_id),
        })?
        .items
        .into_iter()
        .find(|agent| agent.active.name == params.name)
        .ok_or_else(|| not_found("agent", &params.name))
    }

    pub fn write_agent(
        &mut self,
        params: AgentWriteParams,
    ) -> Result<AgentDefinition, RuntimeError> {
        let target_dir = self.agent_writable_directory(&params.thread_id, params.source)?;
        let mut definition = params.definition;
        definition.name = definition.name.trim().to_string();
        let slug = agent_name_slug(&definition.name)?;
        let filename = format!("{slug}.md");
        let path = target_dir.join(&filename);
        if path.exists() && !params.overwrite {
            return Err(already_exists("agent", &definition.name));
        }

        definition.source = Some(params.source);
        definition.path = Some(path.display().to_string());
        definition.filename = Some(filename);
        definition.read_only = false;

        fs::create_dir_all(&target_dir).map_err(|error| {
            RuntimeError::new(
                "agent_directory_create_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!(
                    "failed to create agent directory {}: {error}",
                    target_dir.display()
                ),
            )
        })?;
        fs::write(&path, oppi_agents::format_agent_markdown(&definition)).map_err(|error| {
            RuntimeError::new(
                "agent_write_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!("failed to write agent {}: {error}", path.display()),
            )
        })?;

        Ok(definition)
    }

    pub fn delete_agent(&mut self, params: AgentDeleteParams) -> Result<(), RuntimeError> {
        let target_dir = self.agent_writable_directory(&params.thread_id, params.source)?;
        let target = if let Some(definition) =
            self.find_agent_definition_for_source(&params.thread_id, params.source, &params.name)?
        {
            if let Some(path) = definition.path {
                PathBuf::from(path)
            } else {
                let slug = agent_name_slug(&params.name)?;
                target_dir.join(format!("{slug}.md"))
            }
        } else {
            let slug = agent_name_slug(&params.name)?;
            target_dir.join(format!("{slug}.md"))
        };
        if !target.exists() {
            return Err(not_found("agent", &params.name));
        }
        fs::remove_file(&target).map_err(|error| {
            RuntimeError::new(
                "agent_delete_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!("failed to delete agent {}: {error}", target.display()),
            )
        })?;
        Ok(())
    }

    pub fn list_skills(
        &self,
        params: SkillListParams,
    ) -> Result<RuntimeListResult<ResolvedSkill>, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let items = resolve_skill_candidates(self.discover_skill_candidates(cwd.as_deref()))
            .into_iter()
            .map(|(active, shadowed)| ResolvedSkill {
                active: active.reference,
                shadowed,
            })
            .collect();
        Ok(RuntimeListResult { items })
    }

    pub fn expand_skill_command(
        &self,
        params: SkillExpandParams,
    ) -> Result<SkillExpandResult, RuntimeError> {
        let Some((name, args)) = parse_skill_command_input(&params.input) else {
            return Ok(SkillExpandResult {
                input: params.input,
                skill: None,
            });
        };
        let cwd = self.thread(&params.thread_id)?.project.cwd.clone();
        let Some((active, _)) =
            resolve_skill_candidates(self.discover_skill_candidates(Some(&cwd)))
                .into_iter()
                .find(|(active, _)| active.reference.name == name)
        else {
            return Ok(SkillExpandResult {
                input: params.input,
                skill: None,
            });
        };
        let input = expand_skill_command_text(&active, args);
        Ok(SkillExpandResult {
            input,
            skill: Some(active.reference),
        })
    }

    pub fn list_prompt_templates(
        &self,
        params: PromptTemplateListParams,
    ) -> Result<RuntimeListResult<ResolvedPromptTemplate>, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let items = resolve_prompt_template_candidates(
            self.discover_prompt_template_candidates(cwd.as_deref()),
        )
        .into_iter()
        .map(|(active, shadowed)| ResolvedPromptTemplate {
            active: active.reference,
            shadowed,
        })
        .collect();
        Ok(RuntimeListResult { items })
    }

    pub fn expand_prompt_template(
        &self,
        params: PromptTemplateExpandParams,
    ) -> Result<PromptTemplateExpandResult, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let requested = normalize_runtime_command(&params.name);
        let Some((candidate, _)) = resolve_prompt_template_candidates(
            self.discover_prompt_template_candidates(cwd.as_deref()),
        )
        .into_iter()
        .find(|(candidate, _)| candidate.reference.name.eq_ignore_ascii_case(&requested)) else {
            return Err(RuntimeError::new(
                "prompt_template_not_found",
                RuntimeErrorCategory::InvalidRequest,
                format!("prompt template not found: {requested}"),
            ));
        };
        let args = split_prompt_template_args(&params.args);
        let mut input = render_prompt_template_body(&candidate.body, &args);
        if let Some(append) = params
            .prompt_variant_append
            .as_deref()
            .filter(|append| !append.trim().is_empty())
        {
            input.push_str("\n\n");
            input.push_str(append);
        }
        Ok(PromptTemplateExpandResult {
            name: candidate.reference.name.clone(),
            input,
            template: candidate.reference,
            notes: vec!["Expanded native prompt template from static prompt resource".to_string()],
        })
    }

    pub fn list_themes(
        &self,
        params: ThemeListParams,
    ) -> Result<RuntimeListResult<ResolvedTheme>, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let items = resolve_theme_candidates(self.discover_theme_candidates(cwd.as_deref()))
            .into_iter()
            .map(|(active, shadowed)| ResolvedTheme {
                active: active.reference,
                shadowed,
            })
            .collect();
        Ok(RuntimeListResult { items })
    }

    pub fn load_theme(&self, params: ThemeLoadParams) -> Result<ThemeLoadResult, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let requested = canonical_builtin_theme_name(&params.name);
        let Some((candidate, _)) =
            resolve_theme_candidates(self.discover_theme_candidates(cwd.as_deref()))
                .into_iter()
                .find(|(candidate, _)| candidate.reference.name.eq_ignore_ascii_case(&requested))
        else {
            return Err(RuntimeError::new(
                "theme_not_found",
                RuntimeErrorCategory::InvalidRequest,
                format!("theme not found: {requested}"),
            ));
        };
        Ok(ThemeLoadResult {
            theme: candidate.reference,
            colors: candidate.colors,
            export: candidate.export,
        })
    }

    pub fn dispatch_agent(
        &mut self,
        params: AgentDispatchParams,
    ) -> Result<AgentDispatchResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let agent = self.active_agent(Some(&params.thread_id), &params.agent_name)?;
        let tool_allowlist = if params.tool_allowlist.is_empty() {
            agent.tools.clone()
        } else {
            params.tool_allowlist.clone()
        };
        let permission_mode = params.permission_mode.or(agent.permission_mode);
        let worktree_root = params.worktree_root.or(agent.worktree_root);
        self.next_agent_run += 1;
        let run = AgentRun {
            id: format!("agent-run-{}", self.next_agent_run),
            thread_id: params.thread_id.clone(),
            parent_thread_id: None,
            child_thread_id: None,
            child_turn_id: None,
            agent_name: agent.name,
            status: AgentRunStatus::Running,
            task: params.task,
            worktree_root: worktree_root.clone(),
            execution_root: worktree_root,
            background: params.background || agent.background,
            role: params.role,
            model: params.model.or(agent.model),
            effort: params.effort.or(agent.effort),
            permission_mode,
            memory_mode: params.memory_mode,
            tool_allowlist,
            tool_denylist: params.tool_denylist,
            isolation: params.isolation,
            color: params.color,
            skills: params.skills,
            max_turns: params.max_turns,
        };
        self.agent_runs.insert(run.id.clone(), run.clone());
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentStarted { run: run.clone() },
        );
        Ok(AgentDispatchResult {
            run,
            events: vec![event],
        })
    }

    pub fn block_agent(
        &mut self,
        params: AgentBlockParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.agent_run_in_thread(&params.thread_id, &params.run_id)?;
        let run = self.agent_run_mut(&params.run_id)?;
        run.status = AgentRunStatus::Blocked;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentBlocked {
                run_id: params.run_id,
                reason: params.reason,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn complete_agent(
        &mut self,
        params: AgentCompleteParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.agent_run_in_thread(&params.thread_id, &params.run_id)?;
        let run = self.agent_run_mut(&params.run_id)?;
        run.status = AgentRunStatus::Completed;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentCompleted {
                run_id: params.run_id,
                output: params.output,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn poll_agent_run(
        &mut self,
        params: AgentRunPollParams,
    ) -> Result<AgentRunPollResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.agent_run_in_thread(&params.thread_id, &params.run_id)?;
        if let Some(request) = self.pending_background_agents.remove(&params.run_id) {
            return self.run_pending_background_agent(request);
        }
        Ok(AgentRunPollResult {
            run: self.agent_run(&params.run_id)?.clone(),
            events: Vec::new(),
            output: self.agent_run_completed_output(&params.thread_id, &params.run_id),
        })
    }

    pub fn resume_agent_run(
        &mut self,
        params: AgentRunResumeParams,
    ) -> Result<AgentRunPollResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let run = self
            .agent_run_in_thread(&params.thread_id, &params.run_id)?
            .clone();
        if run.status != AgentRunStatus::Blocked {
            return Err(RuntimeError::new(
                "agent_run_not_blocked",
                RuntimeErrorCategory::TerminalState,
                format!("agent run {} is not blocked", params.run_id),
            ));
        }
        let state = self
            .background_agent_resumes
            .get(&params.run_id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    "agent_run_not_resumable",
                    RuntimeErrorCategory::InvalidRequest,
                    format!(
                        "agent run {} has no resumable background child turn",
                        params.run_id
                    ),
                )
            })?;
        let child_result = self.resume_agentic_turn_with_registry(
            AgenticTurnResumeParams {
                thread_id: state.child_thread_id.clone(),
                turn_id: state.child_turn_id.clone(),
                follow_up: params.follow_up,
                ask_user_response: params.ask_user_response,
                sandbox_policy: params.sandbox_policy,
                model_steps: params.model_steps,
                model_provider: params.model_provider,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: params.approved_tool_call_ids,
                cancellation: params.cancellation,
                max_continuations: params.max_continuations,
            },
            Some(tool_registry_from_definitions(
                state.tool_definitions.clone(),
            )),
        )?;
        self.finish_background_agent_child_result(
            params.run_id,
            state.parent_thread_id,
            state.child_thread_id,
            state.execution_root,
            state.tool_definitions,
            run.agent_name,
            child_result,
        )
    }

    fn agent_run_completed_output(&self, thread_id: &str, run_id: &str) -> Option<String> {
        self.event_store
            .list_thread(thread_id)
            .ok()?
            .into_iter()
            .rev()
            .find_map(|event| match event.kind {
                EventKind::AgentCompleted {
                    run_id: completed_run_id,
                    output,
                } if completed_run_id == run_id => Some(output),
                _ => None,
            })
    }

    pub fn take_pending_background_agent_requests(&mut self) -> Vec<BackgroundAgentRequest> {
        let pending = std::mem::take(&mut self.pending_background_agents);
        pending.into_values().collect()
    }

    pub fn run_pending_background_agent(
        &mut self,
        mut request: BackgroundAgentRequest,
    ) -> Result<AgentRunPollResult, RuntimeError> {
        let agent_name = self.agent_run(&request.run_id)?.agent_name.clone();
        request.input.execution_mode = ExecutionMode::Blocking;
        let run_id = request.run_id.clone();
        let parent_thread_id = request.parent_thread_id.clone();
        let child_thread_id = request.child_thread_id.clone();
        let execution_root = request.execution_root.clone();
        let tool_definitions = request.tool_definitions.clone();
        let child_result = self.run_agentic_turn_with_parent(
            request.input,
            Some(request.parent_turn_id.clone()),
            Some(tool_registry_from_definitions(tool_definitions.clone())),
        );
        let child = match child_result {
            Ok(child) => child,
            Err(error) => {
                self.background_agent_resumes.remove(&run_id);
                if let Some(run) = self.agent_runs.get_mut(&run_id) {
                    run.status = AgentRunStatus::Failed;
                }
                return Err(error);
            }
        };
        self.finish_background_agent_child_result(
            run_id,
            parent_thread_id,
            child_thread_id,
            execution_root,
            tool_definitions,
            agent_name,
            child,
        )
    }

    fn finish_background_agent_child_result(
        &mut self,
        run_id: AgentRunId,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        execution_root: String,
        tool_definitions: Vec<ToolDefinition>,
        agent_name: String,
        child: AgenticTurnResult,
    ) -> Result<AgentRunPollResult, RuntimeError> {
        let child_turn_id = child.turn.id.clone();
        if let Some(stored) = self.agent_runs.get_mut(&run_id) {
            stored.child_thread_id = Some(child_thread_id.clone());
            stored.child_turn_id = Some(child_turn_id.clone());
            stored.execution_root = Some(execution_root.clone());
        }
        let mut events = child.events.clone();
        let mut output = None;

        if let Some(approval) = child.awaiting_approval {
            let reason = format!(
                "background subagent {} blocked waiting for approval {}",
                agent_name, approval.id
            );
            let blocked = self.block_agent(AgentBlockParams {
                thread_id: parent_thread_id.clone(),
                run_id: run_id.clone(),
                reason,
            })?;
            self.remember_background_agent_resume_state(
                &run_id,
                parent_thread_id,
                child_thread_id,
                child_turn_id,
                execution_root,
                tool_definitions.clone(),
            );
            events.extend(blocked.events);
        } else if let Some(question) = child.awaiting_question {
            let reason = format!(
                "background subagent {} blocked waiting for user input {}",
                agent_name, question.id
            );
            let blocked = self.block_agent(AgentBlockParams {
                thread_id: parent_thread_id.clone(),
                run_id: run_id.clone(),
                reason,
            })?;
            self.remember_background_agent_resume_state(
                &run_id,
                parent_thread_id,
                child_thread_id,
                child_turn_id,
                execution_root,
                tool_definitions.clone(),
            );
            events.extend(blocked.events);
        } else if child.turn.status != TurnStatus::Completed {
            let reason = format!(
                "background subagent {} ended with status {:?}",
                agent_name, child.turn.status
            );
            let blocked = self.block_agent(AgentBlockParams {
                thread_id: parent_thread_id.clone(),
                run_id: run_id.clone(),
                reason,
            })?;
            self.remember_background_agent_resume_state(
                &run_id,
                parent_thread_id,
                child_thread_id,
                child_turn_id,
                execution_root,
                tool_definitions.clone(),
            );
            events.extend(blocked.events);
        } else {
            let completed_output =
                subagent_output_from_events(&child.events).unwrap_or_else(|| {
                    format!(
                        "Subagent {} completed {} without assistant text",
                        agent_name, child.turn.id
                    )
                });
            let completed = self.complete_agent(AgentCompleteParams {
                thread_id: parent_thread_id.clone(),
                run_id: run_id.clone(),
                output: completed_output.clone(),
            })?;
            self.background_agent_resumes.remove(&run_id);
            output = Some(completed_output);
            events.extend(completed.events);
        }

        Ok(AgentRunPollResult {
            run: self.agent_run(&run_id)?.clone(),
            events,
            output,
        })
    }

    fn remember_background_agent_resume_state(
        &mut self,
        run_id: &str,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        child_turn_id: TurnId,
        execution_root: String,
        tool_definitions: Vec<ToolDefinition>,
    ) {
        self.background_agent_resumes.insert(
            run_id.to_string(),
            BackgroundAgentResumeState {
                parent_thread_id,
                child_thread_id,
                child_turn_id,
                execution_root,
                tool_definitions,
            },
        );
    }

    pub fn side_question(
        &mut self,
        params: SideQuestionParams,
    ) -> Result<SideQuestionResult, RuntimeError> {
        let parent_thread_id = self.thread(&params.thread_id)?.id.clone();
        let question = params.question;
        let parent_turn_id = params.parent_turn_id;
        let side = self.fork_thread(
            &parent_thread_id,
            Some(side_question_title_preview(&question)),
        )?;
        let side_thread_id = side.thread.id.clone();
        let turn_result = self.run_agentic_turn_with_parent(
            AgenticTurnParams {
                thread_id: side_thread_id.clone(),
                input: side_question_turn_input(&question),
                execution_mode: params.execution_mode,
                follow_up: None,
                sandbox_policy: params.sandbox_policy,
                model_steps: params.model_steps,
                model_provider: params.model_provider,
                tool_definitions: params.tool_definitions,
                approved_tool_call_ids: params.approved_tool_call_ids,
                cancellation: params.cancellation,
                max_continuations: params.max_continuations,
            },
            parent_turn_id,
            None,
        )?;
        let side_turn_id = turn_result.turn.id.clone();
        let status = turn_result.turn.status;
        let answer =
            subagent_output_from_events(&turn_result.events).unwrap_or_else(|| match status {
                TurnStatus::WaitingForApproval => {
                    "Side question is waiting for approval.".to_string()
                }
                TurnStatus::WaitingForUser => {
                    "Side question is waiting for user input.".to_string()
                }
                TurnStatus::Aborted => "Side question was aborted.".to_string(),
                TurnStatus::Failed => "Side question failed.".to_string(),
                _ => String::new(),
            });
        let event = self.event(
            &parent_thread_id,
            None,
            EventKind::SideQuestionAnswered {
                question,
                answer: answer.clone(),
                parent_thread_id: Some(parent_thread_id.clone()),
                side_thread_id: Some(side_thread_id.clone()),
                side_turn_id: Some(side_turn_id.clone()),
                status: Some(status),
            },
        );
        let mut events = side.events;
        events.extend(turn_result.events);
        events.push(event);
        Ok(SideQuestionResult {
            answer,
            parent_thread_id,
            side_thread_id,
            side_turn_id: Some(side_turn_id),
            status: Some(status),
            events,
        })
    }

    pub fn bridge_pi_event(
        &mut self,
        params: PiBridgeEventParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::PiAdapterEvent {
                name: params.name,
                payload: params.payload,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn events_after(&self, params: EventsListParams) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_EVENTS_LIST_LIMIT)
            .min(MAX_EVENTS_LIST_LIMIT);
        let events = self
            .event_store
            .list_after_limit(&params.thread_id, params.after, limit)
            .map_err(event_store_error)?;
        Ok(EventsListResult { events })
    }

    pub fn permission_review_history(
        &self,
        params: PermissionReviewHistoryParams,
    ) -> Result<PermissionReviewHistoryResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let limit = params
            .limit
            .unwrap_or(GUARDIAN_REVIEW_HISTORY_LIMIT)
            .min(GUARDIAN_REVIEW_HISTORY_LIMIT);
        let records = self
            .guardian_review_records
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect();
        Ok(PermissionReviewHistoryResult { records })
    }

    pub fn permission_review_context(
        &self,
        params: PermissionReviewContextParams,
    ) -> Result<PermissionReviewContextResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let mut reviewer_tools = reviewer_tool_definitions(&self.tool_registry)
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();
        let reviewer_tool_order = ["oppi_review_read", "oppi_review_ls", "oppi_review_grep"];
        reviewer_tools.sort_by_key(|name| {
            reviewer_tool_order
                .iter()
                .position(|expected| expected == name)
                .unwrap_or(reviewer_tool_order.len())
        });
        Ok(PermissionReviewContextResult {
            settings: self.guardian_settings.clone(),
            reviewer_provider_policy:
                "isolated non-Meridian OpenAI-compatible/OpenAI-Codex reviewer; malformed output, provider failure, unsafe provider choice, timeout, or unsupported reviewer tool use fails closed"
                    .to_string(),
            prompt_context: vec![
                "strict JSON decision contract".to_string(),
                "current permission mode, working directory, and protected patterns".to_string(),
                "bounded parent system prompt excerpt, truncated to 5000 characters".to_string(),
                "compact runtime transcript for the active thread/turn".to_string(),
                "recent Guardian records, capped to 8 summaries".to_string(),
                "local risk summary from Rust sandbox preflight".to_string(),
                "proposed tool call JSON".to_string(),
            ],
            reviewer_tools,
            hard_denies: vec![
                "protected paths or protected-pattern hits".to_string(),
                "raw secret-bearing arguments".to_string(),
                "read-only mode writes".to_string(),
                "sandbox Deny decisions, out-of-root paths, or disabled-network requirements"
                    .to_string(),
                "input larger than the bounded auto-review argument limit".to_string(),
            ],
            output_contract: json!({
                "outcome": "allow|deny",
                "risk_level": "low|medium|high|critical",
                "user_authorization": "unknown|low|medium|high",
                "rationale": "one concise sentence grounded in the conversation and tool call",
                "cache_scope": "none|exact"
            }),
            prompt_skeleton: format!(
                "{GUARDIAN_REVIEWER_OUTPUT_CONTRACT}\n\n\
                 Review the proposed OPPi tool call for permission auto-review.\n\
                 Current permission mode: <mode>\n\
                 Working directory: <cwd>\n\
                 Protected patterns: <patterns>\n\
                 Current OPPi/Pi system prompt excerpt:\n<bounded parent prompt excerpt>\n\
                 Recent transcript:\n<compact runtime transcript>\n\
                 Recent Guardian records:\n<up to 8 prior review summaries>\n\
                 Local risk summary:\n<risk JSON>\n\
                 Proposed tool call JSON:\n<tool call JSON>\n\n\
                 Strict decision rules:\n\
                 - Return outcome allow only when the user clearly authorized this exact action and the local risk is low enough for auto-review.\n\
                 - Return outcome deny when the action is protected, ambiguous, secret-bearing, outside scope, or would need manual user judgment.\n\
                 - Use cache_scope exact only for low-risk approvals with medium or high authorization and no protected path; otherwise use none.\n\
                 - Reviewer helper tools are limited to oppi_review_read, oppi_review_ls, and oppi_review_grep.\n\n\
                 Return exactly one JSON object. Do not include markdown fences or prose."
            ),
        })
    }

    pub fn permission_review_settings(&self) -> PermissionReviewSettingsResult {
        PermissionReviewSettingsResult {
            settings: self.guardian_settings.clone(),
        }
    }

    pub fn update_permission_review_settings(
        &mut self,
        params: PermissionReviewSettingsUpdateParams,
    ) -> Result<PermissionReviewSettingsResult, RuntimeError> {
        if let Some(thread_id) = params.thread_id.as_deref() {
            self.thread(thread_id)?;
        }
        if params.settings.timeout_ms == 0 {
            return Err(RuntimeError::new(
                "invalid_permission_review_settings",
                RuntimeErrorCategory::InvalidRequest,
                "permission review timeout must be greater than zero",
            ));
        }
        self.guardian_settings = params.settings;
        Ok(self.permission_review_settings())
    }

    pub fn ask_user_settings(&self) -> AskUserSettingsResult {
        AskUserSettingsResult {
            settings: self.ask_user_settings.clone(),
        }
    }

    pub fn update_ask_user_settings(
        &mut self,
        params: AskUserSettingsUpdateParams,
    ) -> Result<AskUserSettingsResult, RuntimeError> {
        if let Some(thread_id) = params.thread_id.as_deref() {
            self.thread(thread_id)?;
        }
        if !ask_user_timeout_ms_is_allowed(params.settings.timeout_ms) {
            return Err(RuntimeError::new(
                "invalid_ask_user_settings",
                RuntimeErrorCategory::InvalidRequest,
                "ask_user timeout must be off, 1, 2, 5, or 10 minutes",
            ));
        }
        self.ask_user_settings = params.settings;
        Ok(self.ask_user_settings())
    }

    pub fn clear_permission_review_session(
        &mut self,
        params: PermissionReviewClearSessionParams,
    ) -> Result<PermissionReviewClearSessionResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let cleared_reviews = self.guardian_review_records.len();
        let cleared_cache_entries = self.guardian_exact_cache.len();
        let cleared_circuit_keys = self.guardian_denial_circuit.len();
        self.guardian_review_records.clear();
        self.guardian_exact_cache.clear();
        self.guardian_denial_circuit.clear();
        Ok(PermissionReviewClearSessionResult {
            cleared_reviews,
            cleared_cache_entries,
            cleared_circuit_keys,
            message: "permission review session state cleared".to_string(),
        })
    }

    pub fn metrics(&self) -> RuntimeMetrics {
        let mut turn_status_counts = BTreeMap::new();
        for turn in self.turns.values() {
            *turn_status_counts
                .entry(turn_status_name(turn.status).to_string())
                .or_insert(0) += 1;
        }
        RuntimeMetrics {
            thread_count: self.threads.len() as u64,
            turn_count: self.turns.len() as u64,
            event_count: self.next_event,
            pending_approvals: self
                .approvals
                .values()
                .filter(|approval| approval.outcome.is_none())
                .count() as u64,
            pending_questions: self
                .questions
                .values()
                .filter(|question| !question.resolved)
                .count() as u64,
            plugin_count: self.plugins.len() as u64,
            mcp_server_count: self.mcp_servers.len() as u64,
            model_count: self.models.len() as u64,
            agent_definition_count: self.agents.values().map(Vec::len).sum::<usize>() as u64,
            agent_run_count: self.agent_runs.len() as u64,
            turn_status_counts,
        }
    }

    pub fn debug_bundle(&self, mut diagnostics: Vec<Diagnostic>) -> DebugBundle {
        diagnostics.extend(self.recent_diagnostics(25));
        DebugBundle {
            schema_version: 1,
            redacted: true,
            metrics: self.metrics(),
            diagnostics,
        }
    }

    fn recent_diagnostics(&self, limit: usize) -> Vec<Diagnostic> {
        let mut diagnostics = self
            .event_store
            .threads()
            .into_iter()
            .flat_map(|thread_id| self.event_store.list_thread(&thread_id).unwrap_or_default())
            .filter_map(|event| match event.kind {
                EventKind::Diagnostic { diagnostic } => Some(diagnostic),
                _ => None,
            })
            .collect::<Vec<_>>();
        diagnostics.reverse();
        diagnostics.truncate(limit);
        diagnostics.reverse();
        diagnostics
    }

    fn turn_has_provider_replay_snapshot(&self, turn: &Turn) -> bool {
        matches!(
            turn.status,
            TurnStatus::WaitingForApproval | TurnStatus::WaitingForUser
        ) && self.direct_provider_turns.contains_key(&turn.id)
    }

    fn clear_pending_pauses_for_turn(&mut self, turn_id: &str) {
        for approval in self.approvals.values_mut() {
            if approval.turn_id.as_deref() == Some(turn_id) && approval.outcome.is_none() {
                approval.outcome = Some(ApprovalOutcome::Cancelled);
            }
        }
        for question in self.questions.values_mut() {
            if question.turn_id.as_deref() == Some(turn_id) {
                question.resolved = true;
            }
        }
    }

    pub fn recover_incomplete_turns(&mut self, reason: impl Into<String>) -> EventsListResult {
        let reason = reason.into();
        let recoverable: Vec<(ThreadId, TurnId)> = self
            .turns
            .values()
            .filter(|turn| !turn_is_terminal(turn))
            .filter(|turn| !self.turn_has_provider_replay_snapshot(turn))
            .map(|turn| (turn.thread_id.clone(), turn.id.clone()))
            .collect();
        let mut events = Vec::new();
        for (thread_id, turn_id) in recoverable {
            if let Some(turn) = self.turns.get_mut(&turn_id) {
                turn.status = TurnStatus::Aborted;
            }
            self.clear_pending_pauses_for_turn(&turn_id);
            events.push(self.event(
                &thread_id,
                Some(&turn_id),
                EventKind::TurnAborted {
                    reason: reason.clone(),
                },
            ));
        }
        EventsListResult { events }
    }

    pub fn replay_events(events: &[Event]) -> Result<Self, RuntimeError> {
        let mut runtime = Runtime::new();
        runtime.threads.clear();
        runtime.turns.clear();
        runtime.approvals.clear();
        runtime.thread_item_labels.clear();
        runtime.thread_fork_points.clear();
        runtime.thread_history_cutoffs.clear();
        runtime.questions.clear();
        runtime.plugins.clear();
        runtime.todos = TodoState::default();
        runtime.goals.clear();
        runtime.goal_active_started_at_ms.clear();
        runtime.goal_continuations.clear();
        runtime.mcp_servers.clear();
        #[cfg(test)]
        runtime.mcp_mock_servers.clear();
        runtime.models.clear();
        runtime.agent_runs.clear();
        runtime.direct_provider_turns.clear();
        runtime.guardian_review_records.clear();
        runtime.guardian_exact_cache.clear();
        runtime.guardian_denial_circuit.clear();
        runtime.selected_model = None;
        runtime.event_store = InMemoryEventStore::new();

        for event in events {
            runtime.next_event = runtime.next_event.max(event.id);
            runtime
                .event_store
                .append(event.clone())
                .map_err(event_store_error)?;
            runtime.apply_replayed_event(event)?;
        }
        Ok(runtime)
    }

    pub fn reserve_thread_counter(&mut self, counter: u64) {
        self.next_thread = self.next_thread.max(counter);
    }

    fn apply_replayed_event(&mut self, event: &Event) -> Result<(), RuntimeError> {
        match &event.kind {
            EventKind::ThreadStarted { thread }
            | EventKind::ThreadResumed { thread }
            | EventKind::ThreadForked { thread, .. }
            | EventKind::ThreadUpdated { thread } => {
                self.bump_thread_counter(&thread.id);
                self.threads.insert(thread.id.clone(), thread.clone());
            }
            EventKind::ThreadForkPointSet {
                thread_id,
                source_event_id,
                history_cutoff_event_id,
                ..
            } => {
                self.thread_fork_points
                    .insert(thread_id.clone(), *source_event_id);
                if let Some(history_cutoff_event_id) = history_cutoff_event_id {
                    self.thread_history_cutoffs
                        .insert(thread_id.clone(), *history_cutoff_event_id);
                }
            }
            EventKind::ThreadGoalUpdated { goal } => {
                self.goals.insert(goal.thread_id.clone(), goal.clone());
                if goal.status == ThreadGoalStatus::Active {
                    self.goal_active_started_at_ms
                        .insert(goal.thread_id.clone(), goal.updated_at_ms);
                } else {
                    self.goal_active_started_at_ms.remove(&goal.thread_id);
                }
            }
            EventKind::ThreadGoalCleared { thread_id } => {
                self.goals.remove(thread_id);
                self.goal_active_started_at_ms.remove(thread_id);
                self.goal_continuations.remove(thread_id);
            }
            EventKind::TurnStarted { turn } => {
                self.bump_turn_counter(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
            }
            EventKind::TurnPhaseChanged { phase } => {
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.phase = *phase;
                }
            }
            EventKind::TurnCompleted { turn_id } => {
                if let Some(turn) = self.turns.get_mut(turn_id) {
                    turn.status = TurnStatus::Completed;
                }
                self.clear_pending_pauses_for_turn(turn_id);
                self.direct_provider_turns.remove(turn_id);
            }
            EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. } => {
                if let Some(turn_id) = event.turn_id.as_deref() {
                    if let Some(turn) = self.turns.get_mut(turn_id) {
                        turn.status = TurnStatus::Aborted;
                    }
                    self.clear_pending_pauses_for_turn(turn_id);
                    self.direct_provider_turns.remove(turn_id);
                }
            }
            EventKind::ApprovalRequested { request } => {
                self.bump_approval_counter(&request.id);
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.status = TurnStatus::WaitingForApproval;
                }
                self.approvals.insert(
                    request.id.clone(),
                    OwnedApprovalRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: event.turn_id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(|call| call.id.clone()),
                        tool_call: request.tool_call.clone(),
                        outcome: None,
                    },
                );
            }
            EventKind::ApprovalResolved { decision } => {
                if let Some(owner) = self.approvals.get_mut(&decision.request_id) {
                    owner.outcome = Some(decision.decision);
                }
            }
            EventKind::PermissionReviewStarted { record }
            | EventKind::PermissionReviewCompleted { record } => {
                self.guardian_remember_review_record(record.clone());
            }
            EventKind::QuestionRequested { request } => {
                self.bump_question_counter(&request.id);
                self.questions.insert(
                    request.id.clone(),
                    OwnedQuestionRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: None,
                        tool_call_id: None,
                        tool_call: None,
                        resolved: false,
                    },
                );
            }
            EventKind::QuestionResolved { response } => {
                if let Some(owner) = self.questions.get_mut(&response.request_id) {
                    owner.resolved = true;
                }
            }
            EventKind::AskUserRequested { request } => {
                self.bump_question_counter(&request.id);
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.status = TurnStatus::WaitingForUser;
                }
                let tool_call = event
                    .turn_id
                    .as_deref()
                    .and_then(|turn_id| self.direct_provider_turns.get(turn_id))
                    .and_then(|state| {
                        request
                            .tool_call_id
                            .as_deref()
                            .and_then(|call_id| tool_call_from_provider_snapshot(state, call_id))
                    });
                self.questions.insert(
                    request.id.clone(),
                    OwnedQuestionRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: event.turn_id.clone(),
                        tool_call_id: request.tool_call_id.clone(),
                        tool_call,
                        resolved: false,
                    },
                );
            }
            EventKind::AskUserResolved { response } => {
                if let Some(owner) = self.questions.get_mut(&response.request_id) {
                    owner.resolved = true;
                }
            }
            EventKind::ThreadItemLabelsUpdated {
                target_event_id,
                labels,
            } => {
                let labels = normalize_thread_item_labels(labels.clone());
                let key = (event.thread_id.clone(), *target_event_id);
                if labels.is_empty() {
                    self.thread_item_labels.remove(&key);
                } else {
                    self.thread_item_labels.insert(key, labels);
                }
            }
            EventKind::ProviderTranscriptSnapshot { snapshot } => {
                self.direct_provider_turns.insert(
                    snapshot.turn_id.clone(),
                    DirectProviderResumeState::from_replay_snapshot(snapshot),
                );
            }
            EventKind::SuggestionOffered { suggestion } => {
                self.suggested_next = Some(suggestion.clone());
            }
            EventKind::PluginRegistered { plugin } => {
                self.plugins.insert(plugin.id.clone(), plugin.clone());
            }
            EventKind::MemoryStatusChanged { status } => {
                self.memory = status.clone();
            }
            EventKind::MemoryJobUpdated { job } => {
                self.bump_memory_job_counter(&job.id);
                self.memory_jobs.insert(job.id.clone(), job.clone());
                self.refresh_memory_queue_depth();
            }
            EventKind::TodosUpdated { state } => {
                self.todos = state.clone();
            }
            EventKind::McpServerRegistered { server } => {
                self.mcp_servers.insert(server.id.clone(), server.clone());
            }
            EventKind::ModelRegistered { model } => {
                self.models.insert(model.id.clone(), model.clone());
            }
            EventKind::ModelSelected { model_id } => {
                self.selected_model = Some(model_id.clone());
            }
            EventKind::AgentStarted { run } => {
                self.bump_agent_run_counter(&run.id);
                self.agent_runs.insert(run.id.clone(), run.clone());
            }
            EventKind::AgentBlocked { run_id, .. } => {
                if let Some(run) = self.agent_runs.get_mut(run_id) {
                    run.status = AgentRunStatus::Blocked;
                }
            }
            EventKind::AgentCompleted { run_id, .. } => {
                if let Some(run) = self.agent_runs.get_mut(run_id) {
                    run.status = AgentRunStatus::Completed;
                }
            }
            EventKind::ItemStarted { item } | EventKind::ItemCompleted { item } => {
                self.bump_item_counter(&item.id);
            }
            EventKind::ItemDelta { .. }
            | EventKind::ToolCallStarted { .. }
            | EventKind::ToolCallCompleted { .. }
            | EventKind::ArtifactCreated { .. }
            | EventKind::ToolBatchStarted { .. }
            | EventKind::ToolBatchCompleted { .. }
            | EventKind::SandboxStatus { .. }
            | EventKind::HandoffCompacted { .. }
            | EventKind::PiAdapterEvent { .. }
            | EventKind::SideQuestionAnswered { .. }
            | EventKind::Diagnostic { .. } => {}
        }
        Ok(())
    }

    fn persist_provider_replay_snapshot(
        &mut self,
        turn: &Turn,
        provider: &mut dyn ModelProvider,
        emitted: &mut Vec<Event>,
    ) {
        if let Some(snapshot) = provider.snapshot() {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ProviderTranscriptSnapshot {
                    snapshot: snapshot.replay_snapshot(&turn.id),
                },
            ));
            self.direct_provider_turns.insert(turn.id.clone(), snapshot);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_agentic_turn(
        &mut self,
        mut turn: Turn,
        mut state: AgenticLoopState,
        input: String,
        provider: &mut dyn ModelProvider,
        tool_registry: &ToolRegistry,
        approved_tool_call_ids: BTreeSet<ToolCallId>,
        cancellation: Option<AgenticCancellation>,
        max_continuations: u32,
        mut continuation: u32,
        pending_tool_calls: Vec<ToolCall>,
        mut emitted: Vec<Event>,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        for call in pending_tool_calls {
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            if let Some(reason) = cancellation_tool_reason(&cancellation, &call.id) {
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::ToolCallStarted { call: call.clone() },
                ));
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::ToolCallCompleted {
                        result: ToolResult {
                            call_id: call.id.clone(),
                            status: ToolResultStatus::Aborted,
                            output: None,
                            error: Some(reason.clone()),
                        },
                    },
                ));
                return Ok(self.abort_agentic_turn(turn, reason, emitted));
            }
            self.execute_and_observe_agentic_tool(
                &turn,
                tool_registry,
                provider,
                &call,
                None,
                &mut emitted,
            )?;
        }

        loop {
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }

            if continuation > max_continuations {
                if state.current != TurnPhase::Loop {
                    self.transition_agentic_phase(
                        &mut state,
                        &mut turn,
                        TurnPhase::Loop,
                        &mut emitted,
                    )?;
                }
                turn.status = TurnStatus::Aborted;
                self.direct_provider_turns.remove(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::TurnAborted {
                        reason: "continuation guard exceeded".to_string(),
                    },
                ));
                return Ok(AgenticTurnResult {
                    turn,
                    events: emitted,
                    awaiting_approval: None,
                    awaiting_question: None,
                });
            }

            if let Some(reason) = cancellation_before_model_reason(&cancellation, continuation) {
                return Ok(self.abort_agentic_turn(turn, reason, emitted));
            }

            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Api, &mut emitted)?;
            let request = ModelRequest {
                thread_id: turn.thread_id.clone(),
                turn_id: turn.id.clone(),
                input: input.clone(),
                continuation,
                history: self.compact_provider_history(&turn.thread_id, &turn.id),
            };
            let provider_step = match provider.next_step(&request) {
                Ok(step) => step,
                Err(error) => {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
            };
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            self.emit_provider_diagnostics(&turn, provider_step.diagnostics, &mut emitted);
            self.account_active_goal(
                &turn.thread_id,
                provider_step.known_token_delta,
                0,
                &mut emitted,
            );
            let step = provider_step.step;
            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Tokens, &mut emitted)?;
            self.stream_assistant_deltas(&turn, &step.assistant_deltas, &mut emitted);
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Tools, &mut emitted)?;
            let mut provided_results = match tool_results_by_call_id(&step) {
                Ok(results) => results,
                Err(error) => {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
            };

            for call in step.tool_calls.clone() {
                if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                    return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
                }
                if let Some(reason) = cancellation_tool_reason(&cancellation, &call.id) {
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ToolCallStarted { call: call.clone() },
                    ));
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ToolCallCompleted {
                            result: ToolResult {
                                call_id: call.id.clone(),
                                status: ToolResultStatus::Aborted,
                                output: None,
                                error: Some(reason.clone()),
                            },
                        },
                    ));
                    return Ok(self.abort_agentic_turn(turn, reason, emitted));
                }
                let provided_result = provided_results.remove(&call.id);
                if provided_result.is_none()
                    && lookup_tool(tool_registry, &call)
                        .is_some_and(|definition| is_ask_user_tool(definition, &call))
                {
                    self.next_question += 1;
                    let request_id = format!("question-{}", self.next_question);
                    match normalize_ask_user_request(
                        request_id.clone(),
                        &call,
                        &self.ask_user_settings,
                    ) {
                        Ok(request) => {
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallStarted { call: call.clone() },
                            ));
                            self.questions.insert(
                                request.id.clone(),
                                OwnedQuestionRequest {
                                    thread_id: turn.thread_id.clone(),
                                    turn_id: Some(turn.id.clone()),
                                    tool_call_id: Some(call.id.clone()),
                                    tool_call: Some(call.clone()),
                                    resolved: false,
                                },
                            );
                            self.persist_provider_replay_snapshot(&turn, provider, &mut emitted);
                            self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                            turn.status = TurnStatus::WaitingForUser;
                            self.turns.insert(turn.id.clone(), turn.clone());
                            self.emit_hook_diagnostic(
                                &turn,
                                "user-input",
                                format!("tool {} paused for user input", call.name),
                                &mut emitted,
                            );
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::AskUserRequested {
                                    request: request.clone(),
                                },
                            ));
                            return Ok(AgenticTurnResult {
                                turn,
                                events: emitted,
                                awaiting_approval: None,
                                awaiting_question: Some(request),
                            });
                        }
                        Err((status, error)) => {
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallStarted { call: call.clone() },
                            ));
                            let result = ToolResult {
                                call_id: call.id.clone(),
                                status,
                                output: None,
                                error: Some(error),
                            };
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallCompleted {
                                    result: result.clone(),
                                },
                            ));
                            provider.observe_tool_result(&call, &result)?;
                            continue;
                        }
                    }
                }
                let requires_approval = self.agentic_tool_requires_approval(tool_registry, &call);
                if provided_result.is_none()
                    && requires_approval
                    && !approved_tool_call_ids.contains(&call.id)
                    && !self.has_approved_tool_call(&turn.thread_id, &turn.id, &call.id)
                {
                    if let Some(review) =
                        self.guardian_auto_review_tool_call(&turn, tool_registry, &call)
                    {
                        self.emit_guardian_review_diagnostic(&turn, &review, &mut emitted);
                        let risk = guardian_risk_summary(&call, &review);
                        let reviewing = self.guardian_reviewing_record(&turn, &call, risk.clone());
                        emitted.push(self.event(
                            &turn.thread_id,
                            Some(&turn.id),
                            EventKind::PermissionReviewStarted { record: reviewing },
                        ));
                        let completed = if review.decision == GuardianReviewDecisionKind::Deny {
                            self.guardian_preflight_denied_record(
                                &turn,
                                &call,
                                risk.clone(),
                                &review,
                            )
                        } else if let Some(cached) =
                            self.guardian_cached_record(&guardian_tool_signature(&call))
                        {
                            self.guardian_cached_review_record(&turn, &call, risk.clone(), &cached)
                        } else if self.guardian_circuit_open(&guardian_circuit_key(&call, &risk)) {
                            self.guardian_circuit_blocked_record(&turn, &call, risk.clone())
                        } else {
                            let parent_provider_config = provider.direct_config();
                            self.run_guardian_reviewer(
                                &turn,
                                parent_provider_config.as_ref(),
                                &call,
                                risk.clone(),
                            )
                        };
                        emitted.push(self.event(
                            &turn.thread_id,
                            Some(&turn.id),
                            EventKind::PermissionReviewCompleted {
                                record: completed.clone(),
                            },
                        ));
                        let allowed = matches!(
                            completed.status,
                            GuardianReviewStatus::Approved | GuardianReviewStatus::Cached
                        ) && completed.decision.as_ref().is_some_and(|decision| {
                            decision.outcome == GuardianReviewOutcome::Allow
                        });
                        if allowed {
                            let approved_call = guardian_approved_tool_call(&call);
                            if let Err(error) = self.execute_and_observe_agentic_tool(
                                &turn,
                                tool_registry,
                                provider,
                                &approved_call,
                                None,
                                &mut emitted,
                            ) {
                                let reason = error.message.clone();
                                let _ = self.abort_agentic_turn(turn, reason, emitted);
                                return Err(error);
                            }
                        } else {
                            self.observe_guardian_denied_agentic_tool(
                                &turn,
                                provider,
                                &call,
                                &completed,
                                &mut emitted,
                            )?;
                        }
                        if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                            return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
                        }
                        continue;
                    }
                    self.next_approval += 1;
                    let request = ApprovalRequest {
                        id: format!("approval-{}", self.next_approval),
                        reason: format!("tool {} requires approval", call.name),
                        risk: RiskLevel::Medium,
                        tool_call: Some(call.clone()),
                    };
                    self.approvals.insert(
                        request.id.clone(),
                        OwnedApprovalRequest {
                            thread_id: turn.thread_id.clone(),
                            turn_id: Some(turn.id.clone()),
                            tool_call_id: Some(call.id.clone()),
                            tool_call: Some(call.clone()),
                            outcome: None,
                        },
                    );
                    self.persist_provider_replay_snapshot(&turn, provider, &mut emitted);
                    self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                    turn.status = TurnStatus::WaitingForApproval;
                    self.turns.insert(turn.id.clone(), turn.clone());
                    self.emit_hook_diagnostic(
                        &turn,
                        "policy",
                        format!("tool {} paused for approval", call.name),
                        &mut emitted,
                    );
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ApprovalRequested {
                            request: request.clone(),
                        },
                    ));
                    return Ok(AgenticTurnResult {
                        turn,
                        events: emitted,
                        awaiting_approval: Some(request),
                        awaiting_question: None,
                    });
                }
                if let Err(error) = self.execute_and_observe_agentic_tool(
                    &turn,
                    tool_registry,
                    provider,
                    &call,
                    provided_result,
                    &mut emitted,
                ) {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
                if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                    return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
                }
            }

            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }

            if !provided_results.is_empty() {
                let error = RuntimeError::new(
                    "tool_result_without_call",
                    RuntimeErrorCategory::ToolPairing,
                    format!(
                        "model step provided tool results without matching calls: {}",
                        provided_results
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                let reason = error.message.clone();
                let _ = self.abort_agentic_turn(turn, reason, emitted);
                return Err(error);
            }

            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Loop, &mut emitted)?;
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            if step.final_response || step.tool_calls.is_empty() {
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Render,
                    &mut emitted,
                )?;
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Hooks,
                    &mut emitted,
                )?;
                self.emit_hook_diagnostic(
                    &turn,
                    "stop",
                    "agentic turn reached final response".to_string(),
                    &mut emitted,
                );
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Await,
                    &mut emitted,
                )?;
                turn.status = TurnStatus::Completed;
                self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                self.direct_provider_turns.remove(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::TurnCompleted {
                        turn_id: turn.id.clone(),
                    },
                ));
                return Ok(AgenticTurnResult {
                    turn,
                    events: emitted,
                    awaiting_approval: None,
                    awaiting_question: None,
                });
            }
            continuation += 1;
        }
    }

    fn abort_agentic_turn(
        &mut self,
        mut turn: Turn,
        reason: String,
        mut emitted: Vec<Event>,
    ) -> AgenticTurnResult {
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        self.finish_goal_turn_accounting(&turn.thread_id, true, &mut emitted);
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnAborted { reason },
        ));
        AgenticTurnResult {
            turn,
            events: emitted,
            awaiting_approval: None,
            awaiting_question: None,
        }
    }

    fn take_requested_interrupt(&self, turn_id: &str) -> Option<String> {
        self.turn_interrupts.take(turn_id)
    }

    fn interrupt_agentic_turn(
        &mut self,
        mut turn: Turn,
        reason: String,
        mut emitted: Vec<Event>,
    ) -> AgenticTurnResult {
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        self.finish_goal_turn_accounting(&turn.thread_id, true, &mut emitted);
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnInterrupted { reason },
        ));
        AgenticTurnResult {
            turn,
            events: emitted,
            awaiting_approval: None,
            awaiting_question: None,
        }
    }

    fn turn_tool_registry(&self, definitions: Vec<ToolDefinition>) -> ToolRegistry {
        let mut registry = self.tool_registry.clone();
        for server in self.mcp_servers.values() {
            if server.status != McpServerStatus::Enabled || !server.trusted {
                continue;
            }
            for tool in &server.tool_refs {
                let definition = mcp_tool_definition(tool);
                if registry
                    .get(definition.namespace.as_deref(), &definition.name)
                    .is_none()
                {
                    registry.register(definition);
                }
            }
        }
        for tools in self.plugin_runner_tools.values() {
            for tool in tools {
                let definition = plugin_runner_tool_definition(tool);
                if registry
                    .get(definition.namespace.as_deref(), &definition.name)
                    .is_none()
                {
                    registry.register(definition);
                }
            }
        }
        for definition in definitions {
            registry.register(definition);
        }
        registry
    }

    fn discover_skill_candidates(&self, cwd: Option<&str>) -> Vec<SkillCandidate> {
        let mut candidates = builtin_skill_candidates();
        candidates.extend(file_skill_candidates(user_skill_roots()));
        if let Some(cwd) = cwd {
            candidates.extend(file_skill_candidates(project_skill_roots(cwd)));
        }
        candidates.extend(plugin_skill_candidates(cwd));
        candidates
    }

    fn discover_prompt_template_candidates(
        &self,
        cwd: Option<&str>,
    ) -> Vec<PromptTemplateCandidate> {
        let mut candidates = file_prompt_template_candidates(user_prompt_roots());
        if let Some(cwd) = cwd {
            candidates.extend(file_prompt_template_candidates(project_prompt_roots(cwd)));
        }
        candidates.extend(plugin_prompt_template_candidates(cwd));
        candidates
    }

    fn discover_theme_candidates(&self, cwd: Option<&str>) -> Vec<ThemeCandidate> {
        let mut candidates = builtin_theme_candidates();
        candidates.extend(file_theme_candidates(user_theme_roots()));
        if let Some(cwd) = cwd {
            candidates.extend(file_theme_candidates(project_theme_roots(cwd)));
        }
        candidates.extend(plugin_theme_candidates(cwd));
        candidates
    }

    fn selected_skill_candidates(
        &self,
        thread_id: &str,
        input: &str,
        explicit_names: &[String],
    ) -> Result<Vec<SkillCandidate>, RuntimeError> {
        let cwd = self.thread(thread_id)?.project.cwd.clone();
        Ok(
            resolve_skill_candidates(self.discover_skill_candidates(Some(&cwd)))
                .into_iter()
                .map(|(active, _)| active)
                .filter(|skill| skill_relevant(&skill.reference, input, explicit_names))
                .collect(),
        )
    }

    fn skill_injection_prompt(
        &self,
        thread_id: &str,
        input: &str,
        explicit_names: &[String],
    ) -> Result<Option<(String, Vec<SkillRef>)>, RuntimeError> {
        let selected = self.selected_skill_candidates(thread_id, input, explicit_names)?;
        if selected.is_empty() {
            return Ok(None);
        }
        let mut prompt = String::from(
            "The following skill instructions were selected deterministically by OPPi. Use them only when they are relevant to the current task.\n",
        );
        let mut refs = Vec::new();
        for skill in selected {
            if prompt.chars().count() >= MAX_SKILL_INJECTION_CHARS {
                break;
            }
            prompt.push_str("\n---\n");
            prompt.push_str(&skill_injection_section(&skill));
            refs.push(skill.reference);
        }
        Ok(Some((
            truncate_chars(&prompt, MAX_SKILL_INJECTION_CHARS),
            refs,
        )))
    }

    fn pending_approved_tool_calls(
        &self,
        thread_id: &str,
        turn_id: &str,
        approved: &BTreeSet<ToolCallId>,
    ) -> Vec<ToolCall> {
        self.approvals
            .values()
            .filter(|approval| {
                approval.thread_id == thread_id
                    && approval.turn_id.as_deref() == Some(turn_id)
                    && approval.outcome == Some(ApprovalOutcome::Approved)
                    && approval
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|call_id| approved.contains(call_id))
            })
            .filter_map(|approval| approval.tool_call.clone())
            .collect()
    }

    fn compact_provider_history(
        &self,
        thread_id: &str,
        current_turn_id: &str,
    ) -> Vec<ProviderHistoryMessage> {
        let mut messages = Vec::new();
        let mut summaries = Vec::new();
        for event in self.provider_history_events(thread_id) {
            match event.kind {
                EventKind::HandoffCompacted { summary, .. } if !summary.trim().is_empty() => {
                    summaries.push(summary);
                }
                EventKind::ItemCompleted { item } => match item.kind {
                    ItemKind::UserMessage { text } if !text.trim().is_empty() => {
                        messages.push(ProviderHistoryMessage {
                            role: ProviderMessageRole::User,
                            content: compact_history_text(&text),
                            turn_id: Some(item.turn_id),
                        });
                    }
                    ItemKind::AssistantMessage { text } if !text.trim().is_empty() => {
                        messages.push(ProviderHistoryMessage {
                            role: ProviderMessageRole::Assistant,
                            content: compact_history_text(&text),
                            turn_id: Some(item.turn_id),
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let mut compact = Vec::new();
        if let Some(summary) = summaries.last() {
            compact.push(ProviderHistoryMessage {
                role: ProviderMessageRole::System,
                content: format!(
                    "Prior compacted conversation summary:\n{}",
                    compact_history_text(summary)
                ),
                turn_id: None,
            });
        }

        let mut total_chars = compact
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
        let mut tail = Vec::new();
        for message in messages.into_iter().rev() {
            let must_keep_current_user = message.role == ProviderMessageRole::User
                && message.turn_id.as_deref() == Some(current_turn_id);
            if !must_keep_current_user
                && (tail.len() >= PROVIDER_HISTORY_MAX_MESSAGES
                    || total_chars + message.content.len() > PROVIDER_HISTORY_MAX_CHARS)
            {
                continue;
            }
            total_chars += message.content.len();
            tail.push(message);
        }
        tail.reverse();
        compact.extend(tail);
        compact
    }

    fn provider_history_events(&self, thread_id: &str) -> Vec<Event> {
        let mut events = Vec::new();
        let mut seen = BTreeSet::new();
        self.collect_provider_history_events(thread_id, None, &mut seen, &mut events);
        events
    }

    fn collect_provider_history_events(
        &self,
        thread_id: &str,
        cutoff_event_id: Option<EventId>,
        seen: &mut BTreeSet<ThreadId>,
        out: &mut Vec<Event>,
    ) {
        if !seen.insert(thread_id.to_string()) {
            return;
        }
        if let Some(thread) = self.threads.get(thread_id)
            && let Some(parent_thread_id) = thread.forked_from.as_deref()
            && let Some(source_event_id) = self.thread_fork_points.get(thread_id).copied()
        {
            let cutoff_event_id = self
                .thread_history_cutoffs
                .get(thread_id)
                .copied()
                .unwrap_or(source_event_id);
            self.collect_provider_history_events(
                parent_thread_id,
                Some(cutoff_event_id),
                seen,
                out,
            );
        }
        let mut events = self.event_store.list_thread(thread_id).unwrap_or_default();
        if let Some(cutoff_event_id) = cutoff_event_id {
            events.retain(|event| event.id <= cutoff_event_id);
        }
        out.extend(events);
    }

    fn emit_provider_diagnostics(
        &mut self,
        turn: &Turn,
        diagnostics: Vec<Diagnostic>,
        emitted: &mut Vec<Event>,
    ) {
        for diagnostic in diagnostics {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
        }
    }

    fn execute_and_observe_agentic_tool(
        &mut self,
        turn: &Turn,
        tool_registry: &ToolRegistry,
        provider: &mut dyn ModelProvider,
        call: &ToolCall,
        provided_result: Option<ToolResult>,
        emitted: &mut Vec<Event>,
    ) -> Result<(), RuntimeError> {
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallStarted { call: call.clone() },
        ));
        let parent_provider_config = provider.direct_config();
        let outcome = self.execute_agentic_tool(
            turn,
            tool_registry,
            call,
            provided_result,
            parent_provider_config,
        );
        emitted.extend(outcome.side_events);
        let result = outcome.result;
        if result.status != ToolResultStatus::Ok {
            self.emit_hook_diagnostic(
                turn,
                "error-recovery",
                format!("tool {} returned {:?}", call.name, result.status),
                emitted,
            );
        }
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallCompleted {
                result: result.clone(),
            },
        ));
        if let Some(artifact) = artifact_metadata_from_tool_result(call, &result) {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ArtifactCreated { artifact },
            ));
        }
        if result.status == ToolResultStatus::Ok
            && lookup_tool(tool_registry, call)
                .is_some_and(|definition| is_suggest_next_tool(definition, call))
            && let Some(suggestion) = self.suggested_next.clone()
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::SuggestionOffered { suggestion },
            ));
        }
        if result.status == ToolResultStatus::Ok
            && lookup_tool(tool_registry, call)
                .is_some_and(|definition| is_todo_tool(definition, call))
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::TodosUpdated {
                    state: self.todos.clone(),
                },
            ));
        }
        provider.observe_tool_result(call, &result)
    }

    fn observe_guardian_denied_agentic_tool(
        &mut self,
        turn: &Turn,
        provider: &mut dyn ModelProvider,
        call: &ToolCall,
        record: &GuardianReviewRecord,
        emitted: &mut Vec<Event>,
    ) -> Result<(), RuntimeError> {
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallStarted { call: call.clone() },
        ));
        let error = record
            .decision
            .as_ref()
            .map(|decision| decision.rationale.clone())
            .unwrap_or_else(|| record.summary.clone());
        let result = ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Denied,
            output: record.strict_json.clone(),
            error: Some(error),
        };
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallCompleted {
                result: result.clone(),
            },
        ));
        provider.observe_tool_result(call, &result)
    }

    fn transition_agentic_phase(
        &mut self,
        state: &mut AgenticLoopState,
        turn: &mut Turn,
        phase: TurnPhase,
        emitted: &mut Vec<Event>,
    ) -> Result<(), RuntimeError> {
        state.transition(phase)?;
        self.set_phase(turn, phase, emitted);
        Ok(())
    }

    fn stream_assistant_deltas(&mut self, turn: &Turn, deltas: &[String], events: &mut Vec<Event>) {
        if deltas.is_empty() {
            return;
        }
        let item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::AssistantMessage {
                text: deltas.concat(),
            },
        );
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted { item: item.clone() },
        ));
        for delta in deltas {
            events.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ItemDelta {
                    item_id: item.id.clone(),
                    delta: delta.clone(),
                },
            ));
        }
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item },
        ));
    }

    fn agentic_tool_requires_approval(&self, registry: &ToolRegistry, call: &ToolCall) -> bool {
        call.arguments
            .get("requireApproval")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || lookup_tool(registry, call).is_some_and(|definition| definition.requires_approval)
    }

    fn execute_agentic_tool(
        &mut self,
        turn: &Turn,
        registry: &ToolRegistry,
        call: &ToolCall,
        provided_result: Option<ToolResult>,
        parent_provider_config: Option<DirectModelProviderConfig>,
    ) -> AgenticToolExecution {
        let Some(definition) = lookup_tool(registry, call) else {
            return AgenticToolExecution::result(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(format!("tool not registered: {}", call.name)),
            });
        };
        if let Some(result) = provided_result {
            return AgenticToolExecution::result(result);
        }
        if mcp_tool_target(definition, call).is_some() {
            return AgenticToolExecution::result(self.execute_mcp_tool(turn, call, definition));
        }
        if plugin_runner_tool_target(definition, call).is_some() {
            return AgenticToolExecution::result(
                self.execute_plugin_runner_tool(turn, call, definition),
            );
        }
        if is_agent_tool(definition, call) {
            return self.execute_agent_tool(turn, registry, call, parent_provider_config);
        }
        if is_todo_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_todo_write_tool(call));
        }
        if is_suggest_next_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_suggest_next_tool(call));
        }
        if is_goal_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_goal_tool(&turn.thread_id, call));
        }
        if is_feedback_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_feedback_tool(turn, call));
        }
        if is_render_mermaid_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_render_mermaid_tool(turn, call));
        }
        if is_shell_task_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_shell_task_tool(call));
        }
        if is_shell_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_shell_tool(turn, call));
        }
        if let Some(kind) = file_tool_kind(definition, call) {
            return AgenticToolExecution::result(self.execute_file_tool(turn, call, kind));
        }
        if is_image_gen_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_image_gen_tool(
                turn,
                call,
                parent_provider_config.as_ref(),
            ));
        }
        if let Some(delay_ms) = tool_delay_ms(call) {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        let output = call
            .arguments
            .get("output")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| Some(call.arguments.to_string()));
        AgenticToolExecution::result(ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Ok,
            output,
            error: None,
        })
    }

    fn preflight_mcp_tool_policy(
        &self,
        turn: &Turn,
        call: &ToolCall,
        definition: &ToolDefinition,
        target: &McpToolTarget,
    ) -> Result<(), (ToolResultStatus, String)> {
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| {
                self.sandbox_policy_for_turn(
                    turn,
                    &cwd,
                    default_file_tool_policy(&cwd, definition.requires_approval),
                )
            });
        let request = guardian_mcp_tool_request(definition, call, &cwd)
            .map_err(|error| (ToolResultStatus::Error, error))?;
        let uses_streamable_http = self
            .mcp_servers
            .get(&target.server_id)
            .and_then(|server| server.transport.as_ref())
            .is_some_and(|transport| {
                matches!(transport, McpTransportConfig::StreamableHttp { .. })
            });
        if uses_streamable_http && policy.network != NetworkPolicy::Enabled {
            return Err((
                ToolResultStatus::Denied,
                format!(
                    "MCP tool {}::{} uses Streamable HTTP transport and requires network policy enabled; current policy is {:?}",
                    target.server_id, target.tool_name, policy.network
                ),
            ));
        }
        if mcp_tool_uses_network(definition) && policy.network != NetworkPolicy::Enabled {
            return Err((
                ToolResultStatus::Denied,
                format!(
                    "MCP tool {}::{} declares openWorldHint and requires network policy enabled; current policy is {:?}",
                    target.server_id, target.tool_name, policy.network
                ),
            ));
        }
        let approval_granted = call
            .arguments
            .get("approvalGranted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match evaluate_exec(&policy, &request) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Deny { reason } => Err((ToolResultStatus::Denied, reason)),
            PolicyDecision::Ask { reason, .. }
                if approval_granted && !reason.contains("protected path") =>
            {
                Ok(())
            }
            PolicyDecision::Ask { reason, .. } => Err((ToolResultStatus::Denied, reason)),
        }
    }

    fn execute_mcp_tool(
        &mut self,
        turn: &Turn,
        call: &ToolCall,
        definition: &ToolDefinition,
    ) -> ToolResult {
        let Some(target) = mcp_tool_target(definition, call) else {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some("tool is not an MCP proxy tool".to_string()),
            };
        };
        let Some(server) = self.mcp_servers.get(&target.server_id).cloned() else {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(format!("MCP server {} is not registered", target.server_id)),
            };
        };
        if server.status != McpServerStatus::Enabled || !server.trusted {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(format!(
                    "MCP server {} is not connected and trusted",
                    target.server_id
                )),
            };
        }
        if let Err((status, error)) =
            self.preflight_mcp_tool_policy(turn, call, definition, &target)
        {
            return ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            };
        }
        let stored_tool = server
            .tool_refs
            .iter()
            .find(|tool| tool.name == target.tool_name);
        let schema = target
            .input_schema
            .as_ref()
            .or_else(|| stored_tool.map(|tool| &tool.input_schema));
        let arguments = mcp_call_arguments(&call.arguments);
        if let Err(error) = validate_mcp_tool_arguments(schema, &arguments) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error),
            };
        }
        let mut transport = match self.mcp_transport_for_server(&server) {
            Ok(transport) => transport,
            Err(error) => {
                let code = error.code.clone();
                let message = error.message.clone();
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: Some(
                        json!({
                            "code": code,
                            "message": message,
                        })
                        .to_string(),
                    ),
                    error: Some(error.message),
                };
            }
        };
        let should_initialize = mcp_transport_should_initialize(&server, &self.mcp_http_sessions);
        let result = (|| -> Result<Value, RuntimeError> {
            if should_initialize {
                initialize_mcp_transport(&mut *transport, "tools")?;
            }
            transport.request(
                "tools/call",
                json!({
                    "name": target.tool_name,
                    "arguments": arguments,
                }),
            )
        })();
        let session_state = transport.session_state();
        let notifications = transport.take_notifications();
        transport.shutdown();
        if let Some(session_state) = session_state {
            self.mcp_http_sessions
                .insert(target.server_id.clone(), session_state);
        }
        self.ingest_mcp_transport_notifications(&turn.thread_id, &target.server_id, notifications);
        match result {
            Ok(result) => {
                let output = mcp_call_result_output(&result);
                let status = if result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    ToolResultStatus::Error
                } else {
                    ToolResultStatus::Ok
                };
                ToolResult {
                    call_id: call.id.clone(),
                    status,
                    output: Some(output.clone()),
                    error: (status == ToolResultStatus::Error).then(|| {
                        result
                            .get("error")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or(output)
                    }),
                }
            }
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "code": error.code.clone(),
                        "message": error.message.clone(),
                    })
                    .to_string(),
                ),
                error: Some(error.message),
            },
        }
    }

    fn execute_plugin_runner_tool(
        &mut self,
        turn: &Turn,
        call: &ToolCall,
        definition: &ToolDefinition,
    ) -> ToolResult {
        let Some(target) = plugin_runner_tool_target(definition, call) else {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some("tool is not a plugin runner proxy tool".to_string()),
            };
        };
        let runner = match self.plugin_runner_for_thread(&turn.thread_id, &target.plugin_id) {
            Ok(runner) => runner,
            Err(error) => {
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Denied,
                    output: None,
                    error: Some(error.message),
                };
            }
        };
        let stored_tool = self
            .plugin_runner_tools
            .get(&target.plugin_id)
            .and_then(|tools| tools.iter().find(|tool| tool.name == target.tool_name));
        let schema = target
            .input_schema
            .as_ref()
            .or_else(|| stored_tool.map(|tool| &tool.input_schema));
        let arguments = mcp_call_arguments(&call.arguments);
        if let Err(error) = validate_mcp_tool_arguments(schema, &arguments) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error.replace("MCP", "plugin runner")),
            };
        }
        let mut transport = match self.plugin_runner_transport_for_runner(&runner) {
            Ok(transport) => transport,
            Err(error) => {
                let code = error.code.clone();
                let message = error.message.clone();
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: Some(
                        json!({
                            "code": code,
                            "message": message,
                        })
                        .to_string(),
                    ),
                    error: Some(error.message),
                };
            }
        };
        let result = transport.request(
            "tools/call",
            json!({
                "name": target.tool_name,
                "arguments": arguments,
            }),
        );
        transport.shutdown();
        match result {
            Ok(result) => {
                let output = mcp_call_result_output(&result);
                let status = if result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    ToolResultStatus::Error
                } else {
                    ToolResultStatus::Ok
                };
                ToolResult {
                    call_id: call.id.clone(),
                    status,
                    output: Some(output.clone()),
                    error: (status == ToolResultStatus::Error).then(|| {
                        result
                            .get("error")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or(output)
                    }),
                }
            }
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "code": error.code.clone(),
                        "message": error.message.clone(),
                    })
                    .to_string(),
                ),
                error: Some(error.message),
            },
        }
    }

    fn execute_agent_tool(
        &mut self,
        parent_turn: &Turn,
        parent_registry: &ToolRegistry,
        call: &ToolCall,
        parent_provider_config: Option<DirectModelProviderConfig>,
    ) -> AgenticToolExecution {
        let mut side_events = Vec::new();
        let result = match self.execute_agent_tool_inner(
            parent_turn,
            parent_registry,
            call,
            parent_provider_config,
            &mut side_events,
        ) {
            Ok(result) => result,
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        };
        AgenticToolExecution {
            result,
            side_events,
        }
    }

    fn execute_agent_tool_inner(
        &mut self,
        parent_turn: &Turn,
        parent_registry: &ToolRegistry,
        call: &ToolCall,
        parent_provider_config: Option<DirectModelProviderConfig>,
        side_events: &mut Vec<Event>,
    ) -> Result<ToolResult, (ToolResultStatus, String)> {
        let agent_name = agent_tool_agent_name(call);
        let task = agent_tool_task(call)?;
        let agent = self
            .active_agent(Some(&parent_turn.thread_id), &agent_name)
            .map_err(runtime_error_as_tool)?;
        let mut policy = resolve_agent_tool_policy(&agent, call)?;
        let mut model_provider = agent_tool_model_provider(call)?.or(parent_provider_config);
        resolve_subagent_model_policy(&mut policy, model_provider.as_ref(), &task);
        let thread = self
            .thread(&parent_turn.thread_id)
            .map_err(runtime_error_as_tool)?
            .clone();
        let parent_root = PathBuf::from(&thread.project.cwd);
        let execution_root = policy
            .worktree_root
            .as_ref()
            .map(|root| resolve_child_worktree_root(&parent_root, root))
            .transpose()?
            .unwrap_or_else(|| lexical_normalize(&parent_root));
        let execution_root_string = execution_root.display().to_string();
        let child_thread_start = self.start_thread(ThreadStartParams {
            project: ProjectRef {
                id: format!("agent:{}:{}", parent_turn.thread_id, agent.name),
                cwd: execution_root_string.clone(),
                display_name: Some(format!("agent:{}", agent.name)),
                workspace_roots: vec![WorkspaceRoot {
                    path: execution_root_string.clone(),
                    label: Some(format!("agent:{}", agent.name)),
                    git_remote: None,
                }],
            },
            title: Some(format!("agent:{}", agent.name)),
        });
        let child_thread = child_thread_start.thread.clone();
        let child_thread_id = child_thread.id.clone();
        let dispatch = self
            .dispatch_agent(AgentDispatchParams {
                thread_id: parent_turn.thread_id.clone(),
                agent_name: agent.name.clone(),
                task: task.clone(),
                worktree_root: policy.worktree_root.clone(),
                background: policy.background,
                role: policy.role.clone(),
                model: policy.model.clone(),
                effort: policy.effort.clone(),
                permission_mode: policy.permission_mode,
                memory_mode: policy.memory_mode.clone(),
                tool_allowlist: policy.tool_allowlist.clone(),
                tool_denylist: policy.tool_denylist.clone(),
                isolation: policy.isolation.clone(),
                color: policy.color.clone(),
                skills: policy.skills.clone(),
                max_turns: policy.max_turns,
            })
            .map_err(runtime_error_as_tool)?;
        let mut run = dispatch.run.clone();
        run.parent_thread_id = Some(parent_turn.thread_id.clone());
        run.child_thread_id = Some(child_thread_id.clone());
        run.execution_root = Some(execution_root_string.clone());
        if let Some(stored) = self.agent_runs.get_mut(&run.id) {
            stored.parent_thread_id = run.parent_thread_id.clone();
            stored.child_thread_id = run.child_thread_id.clone();
            stored.execution_root = run.execution_root.clone();
        }
        let mut dispatch_events = dispatch.events;
        for event in &mut dispatch_events {
            if let EventKind::AgentStarted { run: event_run } = &mut event.kind {
                *event_run = run.clone();
            }
        }
        side_events.extend(dispatch_events);
        side_events.extend(child_thread_start.events);

        let parent_policy = self.sandbox_policy_for_turn(
            parent_turn,
            &thread.project.cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::Default,
                readable_roots: vec![thread.project.cwd.clone()],
                writable_roots: vec![thread.project.cwd.clone()],
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        let child_policy = self.subagent_sandbox_policy(&parent_policy, &policy, &execution_root);
        let child_tools = self.subagent_tool_definitions(parent_registry, &policy);
        let model_steps = agent_tool_model_steps(call)?;
        let subagent_skill_injection = self
            .skill_injection_prompt(&child_thread_id, &task, &policy.skills)
            .map_err(runtime_error_as_tool)?;
        if let Some(config) = model_provider.as_mut() {
            if let Some(model) = policy
                .model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                config.model = model.to_string();
            }
            if let Some(effort) = policy
                .effort
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                config.reasoning_effort = Some(effort.to_string());
            }
            config.system_prompt = Some(subagent_system_prompt(
                &agent,
                &policy,
                config.system_prompt.as_deref(),
                &self.memory,
                subagent_skill_injection
                    .as_ref()
                    .map(|(prompt, _)| prompt.as_str()),
            ));
        }
        let model_steps = if model_steps.is_empty() && model_provider.is_none() {
            vec![ScriptedModelStep {
                assistant_deltas: vec![format!(
                    "Subagent {} completed delegated task in native scripted mode: {}",
                    agent.name, task
                )],
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                final_response: true,
            }]
        } else {
            model_steps
        };

        side_events.push(
            self.event(
                &parent_turn.thread_id,
                Some(&parent_turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "native subagent execution policy applied".to_string(),
                        metadata: BTreeMap::from([
                            ("agent".to_string(), agent.name.clone()),
                            ("runId".to_string(), run.id.clone()),
                            (
                                "permissionMode".to_string(),
                                child_policy.permission_profile.mode.as_str().to_string(),
                            ),
                            ("background".to_string(), policy.background.to_string()),
                            ("toolCount".to_string(), child_tools.len().to_string()),
                            (
                                "memoryMode".to_string(),
                                policy
                                    .memory_mode
                                    .clone()
                                    .unwrap_or_else(|| "inherit".to_string()),
                            ),
                            (
                                "maxTurns".to_string(),
                                policy.max_turns.unwrap_or(3).to_string(),
                            ),
                        ]),
                    },
                },
            ),
        );
        if let Some((_, skills)) = &subagent_skill_injection {
            side_events.push(
                self.event(
                    &parent_turn.thread_id,
                    Some(&parent_turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "subagent skill instructions injected".to_string(),
                            metadata: BTreeMap::from([(
                                "skills".to_string(),
                                skills
                                    .iter()
                                    .map(|skill| skill.name.clone())
                                    .collect::<Vec<_>>()
                                    .join(","),
                            )]),
                        },
                    },
                ),
            );
        }
        let child_params = AgenticTurnParams {
            thread_id: child_thread_id.clone(),
            input: format!(
                "Subagent {} delegated task from {} via {}:\n{}",
                agent.name, parent_turn.id, call.id, task
            ),
            execution_mode: if policy.background {
                ExecutionMode::Background
            } else {
                ExecutionMode::Blocking
            },
            follow_up: None,
            sandbox_policy: Some(child_policy.clone()),
            model_steps,
            model_provider,
            tool_definitions: Vec::new(),
            approved_tool_call_ids: Vec::new(),
            cancellation: None,
            max_continuations: policy.max_turns,
        };
        if policy.background {
            self.pending_background_agents.insert(
                run.id.clone(),
                BackgroundAgentRequest {
                    run_id: run.id.clone(),
                    parent_thread_id: parent_turn.thread_id.clone(),
                    parent_turn_id: parent_turn.id.clone(),
                    child_thread_id: child_thread_id.clone(),
                    child_turn_id: None,
                    execution_root: execution_root_string.clone(),
                    input: child_params,
                    tool_definitions: child_tools.clone(),
                },
            );
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(
                    json!({
                        "status": "running",
                        "runId": run.id,
                        "agentName": agent.name,
                        "threadId": parent_turn.thread_id.clone(),
                        "childThreadId": child_thread_id,
                        "childTurnId": Value::Null,
                        "worktreeRoot": policy.worktree_root,
                        "executionRoot": execution_root_string,
                        "role": policy.role,
                        "model": policy.model,
                        "effort": policy.effort,
                        "permissionMode": child_policy.permission_profile.mode.as_str(),
                        "memoryMode": policy.memory_mode.unwrap_or_else(|| "inherit".to_string()),
                        "background": true,
                        "isolation": policy.isolation,
                        "color": policy.color,
                        "skills": policy.skills,
                        "maxTurns": policy.max_turns,
                    })
                    .to_string(),
                ),
                error: None,
            });
        }

        let child = self
            .run_agentic_turn_with_parent(
                child_params,
                Some(parent_turn.id.clone()),
                Some(tool_registry_from_definitions(child_tools.clone())),
            )
            .map_err(runtime_error_as_tool)?;
        let child_turn_id = child.turn.id.clone();
        run.child_turn_id = Some(child_turn_id.clone());
        if let Some(stored) = self.agent_runs.get_mut(&run.id) {
            stored.child_turn_id = Some(child_turn_id.clone());
        }
        side_events.extend(child.events.clone());

        if let Some(request) = child.awaiting_approval {
            let reason = format!(
                "subagent {} blocked waiting for approval {}",
                agent.name, request.id
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "status": "blocked",
                        "runId": run.id,
                        "agentName": agent.name,
                        "threadId": parent_turn.thread_id.clone(),
                        "childThreadId": child_thread_id.clone(),
                        "childTurnId": child_turn_id.clone(),
                        "worktreeRoot": policy.worktree_root.clone(),
                        "executionRoot": execution_root_string.clone(),
                        "reason": reason,
                        "approvalId": request.id,
                    })
                    .to_string(),
                ),
                error: Some(reason),
            });
        }
        if let Some(request) = child.awaiting_question {
            let reason = format!(
                "subagent {} blocked waiting for user input {}",
                agent.name, request.id
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "status": "blocked",
                        "runId": run.id,
                        "agentName": agent.name,
                        "threadId": parent_turn.thread_id.clone(),
                        "childThreadId": child_thread_id.clone(),
                        "childTurnId": child_turn_id.clone(),
                        "worktreeRoot": policy.worktree_root.clone(),
                        "executionRoot": execution_root_string.clone(),
                        "reason": reason,
                        "questionId": request.id,
                    })
                    .to_string(),
                ),
                error: Some(reason),
            });
        }
        if child.turn.status != TurnStatus::Completed {
            let reason = format!(
                "subagent {} ended with status {:?}",
                agent.name, child.turn.status
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(reason),
            });
        }

        let output = subagent_output_from_events(&child.events).unwrap_or_else(|| {
            format!(
                "Subagent {} completed {} without assistant text",
                agent.name, child.turn.id
            )
        });
        let completed = self
            .complete_agent(AgentCompleteParams {
                thread_id: parent_turn.thread_id.clone(),
                run_id: run.id.clone(),
                output: output.clone(),
            })
            .map_err(runtime_error_as_tool)?;
        side_events.extend(completed.events);
        Ok(ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Ok,
            output: Some(
                json!({
                    "status": "completed",
                    "runId": run.id,
                    "agentName": agent.name,
                    "threadId": parent_turn.thread_id.clone(),
                    "childThreadId": child_thread_id,
                    "childTurnId": child_turn_id,
                    "turnId": child.turn.id,
                    "worktreeRoot": policy.worktree_root,
                    "executionRoot": execution_root_string,
                    "role": policy.role,
                    "model": policy.model,
                    "effort": policy.effort,
                    "permissionMode": child_policy.permission_profile.mode.as_str(),
                    "memoryMode": policy.memory_mode.unwrap_or_else(|| "inherit".to_string()),
                    "background": policy.background,
                    "isolation": policy.isolation,
                    "color": policy.color,
                    "skills": policy.skills,
                    "maxTurns": policy.max_turns,
                    "output": output,
                })
                .to_string(),
            ),
            error: None,
        })
    }

    fn subagent_sandbox_policy(
        &self,
        parent_policy: &SandboxPolicy,
        policy: &ResolvedAgentToolPolicy,
        execution_root: &Path,
    ) -> SandboxPolicy {
        let requested_mode = policy
            .permission_mode
            .unwrap_or(parent_policy.permission_profile.mode);
        let mode = min_permission_mode(parent_policy.permission_profile.mode, requested_mode);
        let requested_network = policy.network_policy.unwrap_or(parent_policy.network);
        let mut child = parent_policy.clone();
        child.permission_profile.mode = mode;
        child.filesystem = filesystem_for_permission(parent_policy.filesystem, mode);
        child.network = min_network_policy(parent_policy.network, requested_network);
        let execution_root = execution_root.display().to_string();
        if policy.worktree_root.is_some() {
            child.permission_profile.readable_roots = vec![execution_root.clone()];
            child.permission_profile.writable_roots =
                if child.filesystem == FilesystemPolicy::ReadOnly {
                    Vec::new()
                } else {
                    vec![execution_root]
                };
        } else {
            child.permission_profile.readable_roots =
                if child.permission_profile.readable_roots.is_empty() {
                    vec![execution_root.clone()]
                } else {
                    child.permission_profile.readable_roots.clone()
                };
            if child.filesystem == FilesystemPolicy::ReadOnly {
                child.permission_profile.writable_roots.clear();
            } else if child.permission_profile.writable_roots.is_empty() {
                child.permission_profile.writable_roots.push(execution_root);
            }
        }
        if child.filesystem == FilesystemPolicy::ReadOnly {
            child.permission_profile.writable_roots.clear();
        }
        child
    }

    fn subagent_tool_definitions(
        &self,
        parent_registry: &ToolRegistry,
        policy: &ResolvedAgentToolPolicy,
    ) -> Vec<ToolDefinition> {
        parent_registry
            .list()
            .into_iter()
            .filter(|definition| {
                agent_tool_definition_allowed(
                    definition,
                    &policy.tool_allowlist,
                    &policy.tool_denylist,
                )
            })
            .collect()
    }

    fn execute_suggest_next_tool(&mut self, call: &ToolCall) -> ToolResult {
        match normalize_suggest_next(call) {
            Ok(Some(suggestion)) => {
                let message = suggestion.message.clone();
                self.suggested_next = Some(suggestion);
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(format!("Suggestion queued for host UI: {}", message)),
                    error: None,
                }
            }
            Ok(None) => {
                self.suggested_next = None;
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(
                        "Suggestion not shown: confidence is below 0.70 or message is empty"
                            .to_string(),
                    ),
                    error: None,
                }
            }
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_goal_tool(&mut self, thread_id: &str, call: &ToolCall) -> ToolResult {
        let result = match call.name.as_str() {
            "get_goal" => self
                .get_thread_goal(ThreadGoalGetParams {
                    thread_id: thread_id.to_string(),
                })
                .map(|result| goal_tool_output(result.goal.as_ref(), None))
                .map_err(|error| error.message),
            "create_goal" => self.create_goal_from_tool(thread_id, call),
            "update_goal" => self.update_goal_from_tool(thread_id, call),
            other => Err(format!("unsupported goal tool: {other}")),
        };
        match result {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error),
            },
        }
    }

    fn create_goal_from_tool(
        &mut self,
        thread_id: &str,
        call: &ToolCall,
    ) -> Result<String, String> {
        if let Some(existing) = self.goals.get(thread_id)
            && existing.status != ThreadGoalStatus::Complete
        {
            return Err(
                "create_goal can only start a goal when no non-complete goal exists".to_string(),
            );
        }
        let objective = first_string_arg(&call.arguments, &["objective"])
            .ok_or_else(|| "create_goal requires an objective string".to_string())?;
        let token_budget = goal_tool_budget_arg(&call.arguments)?;
        let result = self
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.to_string(),
                objective: Some(objective),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(token_budget),
            })
            .map_err(|error| error.message)?;
        Ok(goal_tool_output(Some(&result.goal), None))
    }

    fn update_goal_from_tool(
        &mut self,
        thread_id: &str,
        call: &ToolCall,
    ) -> Result<String, String> {
        let status = first_string_arg(&call.arguments, &["status"])
            .ok_or_else(|| "update_goal requires status".to_string())?
            .to_ascii_lowercase();
        if status != "complete" {
            return Err(
                "update_goal can only mark the existing goal complete; pause, resume, clear, and budget changes are user/runtime controls."
                    .to_string(),
            );
        }
        let result = self
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.to_string(),
                objective: None,
                status: Some(ThreadGoalStatus::Complete),
                token_budget: None,
            })
            .map_err(|error| error.message)?;
        let report = completion_budget_report(&result.goal);
        Ok(goal_tool_output(Some(&result.goal), report))
    }

    fn execute_todo_write_tool(&mut self, call: &ToolCall) -> ToolResult {
        match normalize_todo_write_state(call) {
            Ok(state) => {
                self.todos = state.clone();
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(todo_write_output(&state)),
                    error: None,
                }
            }
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn sandbox_policy_for_turn(
        &self,
        turn: &Turn,
        cwd: &str,
        fallback: SandboxPolicy,
    ) -> SandboxPolicy {
        self.turn_sandbox_policies
            .get(&turn.id)
            .cloned()
            .map(|policy| policy_with_cwd_defaults(policy, cwd))
            .unwrap_or(fallback)
    }

    fn guardian_cached_record(&self, signature: &str) -> Option<GuardianReviewRecord> {
        self.guardian_exact_cache.get(signature).cloned()
    }

    fn guardian_remember_cache(&mut self, record: &GuardianReviewRecord) {
        if guardian_can_cache(record) {
            self.guardian_exact_cache
                .insert(record.signature.clone(), record.clone());
        }
    }

    fn guardian_record_denial(&mut self, circuit_key: &str) {
        self.guardian_record_denial_at(circuit_key, now_millis());
    }

    fn guardian_record_denial_at(&mut self, circuit_key: &str, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(GUARDIAN_DENIAL_CIRCUIT_WINDOW_MS);
        let entries = self
            .guardian_denial_circuit
            .entry(circuit_key.to_string())
            .or_default();
        entries.retain(|timestamp| *timestamp >= cutoff);
        entries.push_back(now_ms);
    }

    fn guardian_circuit_open(&self, circuit_key: &str) -> bool {
        let cutoff = now_millis().saturating_sub(GUARDIAN_DENIAL_CIRCUIT_WINDOW_MS);
        self.guardian_denial_circuit
            .get(circuit_key)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|timestamp| **timestamp >= cutoff)
                    .count()
                    >= GUARDIAN_DENIAL_CIRCUIT_THRESHOLD
            })
            .unwrap_or(false)
    }

    fn guardian_remember_review_record(&mut self, record: GuardianReviewRecord) {
        if let Some(index) = self
            .guardian_review_records
            .iter()
            .position(|existing| existing.id == record.id)
        {
            self.guardian_review_records.remove(index);
        }
        self.guardian_review_records.push_back(record.clone());
        while self.guardian_review_records.len() > GUARDIAN_REVIEW_HISTORY_LIMIT {
            self.guardian_review_records.pop_front();
        }
        self.guardian_remember_cache(&record);
        if guardian_review_counts_as_denial(&record) {
            self.guardian_record_denial(&record.circuit_key);
        }
    }

    fn guardian_review_prompt(
        &self,
        turn: &Turn,
        parent_config: Option<&DirectModelProviderConfig>,
        call: &ToolCall,
        risk: &GuardianRiskSummary,
    ) -> String {
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let policy = self
            .turn_sandbox_policies
            .get(&turn.id)
            .cloned()
            .unwrap_or_else(|| {
                default_policy(PermissionProfile {
                    mode: PermissionMode::AutoReview,
                    readable_roots: vec![cwd.clone()],
                    writable_roots: vec![cwd.clone()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })
            });
        let transcript = self
            .compact_provider_history(&turn.thread_id, &turn.id)
            .into_iter()
            .map(|message| format!("{}: {}", message.role.as_str(), message.content))
            .collect::<Vec<_>>()
            .join("\n");
        let system_prompt_excerpt = parent_config
            .and_then(|config| config.system_prompt.as_deref())
            .map(|prompt| truncate_chars(prompt.trim(), 5_000))
            .filter(|prompt| !prompt.trim().is_empty())
            .unwrap_or_else(|| "(none)".to_string());
        let recent_records = self
            .guardian_review_records
            .iter()
            .rev()
            .take(8)
            .map(|record| {
                format!(
                    "{} {:?} {}",
                    record.id,
                    record.status,
                    record.summary.replace('\n', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let call_json = serde_json::to_string_pretty(call).unwrap_or_else(|_| "{}".to_string());
        let risk_json = serde_json::to_string_pretty(risk).unwrap_or_else(|_| "{}".to_string());
        format!(
            "{GUARDIAN_REVIEWER_OUTPUT_CONTRACT}\n\n\
             Review the proposed OPPi tool call for permission auto-review.\n\
             Current permission mode: {}\n\
             Working directory: {cwd}\n\
             Protected patterns: {}\n\
             Current OPPi/Pi system prompt excerpt:\n{}\n\
             Recent transcript:\n{}\n\
             Recent Guardian records:\n{}\n\
             Local risk summary:\n{risk_json}\n\
             Proposed tool call JSON:\n{call_json}\n\n\
             Strict decision rules:\n\
             - Return outcome allow only when the user clearly authorized this exact action and the local risk is low enough for auto-review.\n\
             - Return outcome deny when the action is protected, ambiguous, secret-bearing, outside scope, or would need manual user judgment.\n\
             - Use risk_level low, medium, high, or critical.\n\
             - Use user_authorization high only for explicit user requests, medium for strong implied authorization, low for weak authorization, unknown when unclear.\n\
             - Use cache_scope exact only for low-risk approvals with medium or high authorization and no protected path; otherwise use none.\n\
             - You may request only oppi_review_read, oppi_review_ls, or oppi_review_grep if bounded read-only project inspection is needed.\n\n\
             Return exactly one JSON object. Do not include markdown fences or prose.",
            policy.permission_profile.mode.as_str(),
            policy.permission_profile.protected_patterns.join(", "),
            system_prompt_excerpt.as_str(),
            if transcript.is_empty() {
                "(none)"
            } else {
                transcript.as_str()
            },
            if recent_records.is_empty() {
                "(none)"
            } else {
                recent_records.as_str()
            }
        )
    }

    fn guardian_review_record_id(&self, turn: &Turn, call: &ToolCall) -> String {
        format!("review-{}-{}", turn.id, call.id)
    }

    fn guardian_reviewing_record(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
    ) -> GuardianReviewRecord {
        GuardianReviewRecord {
            id: self.guardian_review_record_id(turn, call),
            timestamp: now_millis().to_string(),
            status: GuardianReviewStatus::Reviewing,
            mode: PermissionMode::AutoReview,
            tool_name: call.name.clone(),
            tool_call_id: call.id.clone(),
            signature: guardian_tool_signature(call),
            circuit_key: guardian_circuit_key(call, &risk),
            summary: risk.reason.clone(),
            risk,
            decision: None,
            reviewer_model: None,
            cached_from: None,
            strict_json: None,
        }
    }

    fn guardian_review_record_from_decision(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
        reviewer_model: Option<String>,
        decision: oppi_protocol::GuardianReviewDecision,
        strict_json: String,
        status_override: Option<GuardianReviewStatus>,
    ) -> GuardianReviewRecord {
        let status = status_override.unwrap_or(match decision.outcome {
            GuardianReviewOutcome::Allow => GuardianReviewStatus::Approved,
            GuardianReviewOutcome::Deny => GuardianReviewStatus::Denied,
        });
        GuardianReviewRecord {
            id: self.guardian_review_record_id(turn, call),
            timestamp: now_millis().to_string(),
            status,
            mode: PermissionMode::AutoReview,
            tool_name: call.name.clone(),
            tool_call_id: call.id.clone(),
            signature: guardian_tool_signature(call),
            circuit_key: guardian_circuit_key(call, &risk),
            summary: risk.reason.clone(),
            risk,
            decision: Some(decision),
            reviewer_model,
            cached_from: None,
            strict_json: Some(strict_json),
        }
    }

    fn guardian_preflight_denied_record(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
        review: &GuardianReviewResult,
    ) -> GuardianReviewRecord {
        let strict_json = json!({
            "outcome": "deny",
            "risk_level": risk_level_name(review.risk),
            "user_authorization": "unknown",
            "rationale": review.reason,
            "cache_scope": "none",
        })
        .to_string();
        self.guardian_review_record_from_decision(
            turn,
            call,
            risk,
            None,
            oppi_protocol::GuardianReviewDecision {
                outcome: GuardianReviewOutcome::Deny,
                risk_level: review.risk,
                user_authorization: GuardianAuthorizationLevel::Unknown,
                rationale: review.reason.clone(),
                cache_scope: Some(GuardianCacheScope::None),
            },
            strict_json,
            Some(GuardianReviewStatus::Denied),
        )
    }

    fn guardian_cached_review_record(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
        cached: &GuardianReviewRecord,
    ) -> GuardianReviewRecord {
        let mut record = cached.clone();
        record.id = self.guardian_review_record_id(turn, call);
        record.timestamp = now_millis().to_string();
        record.status = GuardianReviewStatus::Cached;
        record.tool_name = call.name.clone();
        record.tool_call_id = call.id.clone();
        record.signature = guardian_tool_signature(call);
        record.circuit_key = guardian_circuit_key(call, &risk);
        record.summary = format!("auto-review cache hit: {}", cached.summary);
        record.risk = risk;
        record.cached_from = Some(cached.id.clone());
        record
    }

    fn guardian_circuit_blocked_record(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
    ) -> GuardianReviewRecord {
        let rationale = "auto-review denied because repeated denials opened the circuit breaker";
        let strict_json = json!({
            "outcome": "deny",
            "risk_level": "high",
            "user_authorization": "unknown",
            "rationale": rationale,
            "cache_scope": "none",
        })
        .to_string();
        self.guardian_review_record_from_decision(
            turn,
            call,
            risk,
            None,
            oppi_protocol::GuardianReviewDecision {
                outcome: GuardianReviewOutcome::Deny,
                risk_level: RiskLevel::High,
                user_authorization: GuardianAuthorizationLevel::Unknown,
                rationale: rationale.to_string(),
                cache_scope: Some(GuardianCacheScope::None),
            },
            strict_json,
            Some(GuardianReviewStatus::CircuitBlocked),
        )
    }

    fn guardian_fail_closed_record(
        &self,
        turn: &Turn,
        call: &ToolCall,
        risk: GuardianRiskSummary,
        reviewer_model: Option<String>,
        reason: impl Into<String>,
    ) -> GuardianReviewRecord {
        let rationale = format!("Auto-review failed closed: {}", reason.into());
        let strict_json = json!({
            "outcome": "deny",
            "risk_level": "critical",
            "user_authorization": "unknown",
            "rationale": rationale,
            "cache_scope": "none",
        })
        .to_string();
        self.guardian_review_record_from_decision(
            turn,
            call,
            risk,
            reviewer_model,
            oppi_protocol::GuardianReviewDecision {
                outcome: GuardianReviewOutcome::Deny,
                risk_level: RiskLevel::Critical,
                user_authorization: GuardianAuthorizationLevel::Unknown,
                rationale,
                cache_scope: Some(GuardianCacheScope::None),
            },
            strict_json,
            Some(GuardianReviewStatus::FailedClosed),
        )
    }

    fn run_guardian_reviewer(
        &mut self,
        turn: &Turn,
        parent_config: Option<&DirectModelProviderConfig>,
        call: &ToolCall,
        risk: GuardianRiskSummary,
    ) -> GuardianReviewRecord {
        match self.run_guardian_reviewer_inner(turn, parent_config, call, risk.clone()) {
            Ok(record) => record,
            Err(error) => self.guardian_fail_closed_record(turn, call, risk, None, error.message),
        }
    }

    fn run_guardian_reviewer_inner(
        &mut self,
        turn: &Turn,
        parent_config: Option<&DirectModelProviderConfig>,
        call: &ToolCall,
        risk: GuardianRiskSummary,
    ) -> Result<GuardianReviewRecord, RuntimeError> {
        let config = guardian_reviewer_provider_config(parent_config, &self.guardian_settings)?;
        let reviewer_model = Some(config.model.clone());
        let reviewer_tools = reviewer_tool_definitions(&self.tool_registry);
        let reviewer_registry = tool_registry_from_definitions(reviewer_tools.clone());
        let mut provider = model_provider(Some(config), Vec::new(), reviewer_tools, None)?;
        let input = self.guardian_review_prompt(turn, parent_config, call, &risk);
        let mut assistant_text = String::new();
        for continuation in 0..GUARDIAN_REVIEWER_MAX_CONTINUATIONS {
            let step = provider.next_step(&ModelRequest {
                thread_id: turn.thread_id.clone(),
                turn_id: turn.id.clone(),
                input: input.clone(),
                continuation,
                history: Vec::new(),
            })?;
            assistant_text.push_str(&step.step.assistant_deltas.concat());
            if step.step.tool_calls.is_empty() {
                let (decision, strict_json) = parse_guardian_review_decision(&assistant_text)
                    .map_err(|error| {
                        RuntimeError::new(
                            "guardian_review_invalid_json",
                            RuntimeErrorCategory::Provider,
                            error,
                        )
                    })?;
                return Ok(self.guardian_review_record_from_decision(
                    turn,
                    call,
                    risk,
                    reviewer_model,
                    decision,
                    strict_json,
                    None,
                ));
            }
            for reviewer_call in step.step.tool_calls {
                let definition =
                    lookup_tool(&reviewer_registry, &reviewer_call).ok_or_else(|| {
                        RuntimeError::new(
                            "guardian_review_tool_not_allowed",
                            RuntimeErrorCategory::Provider,
                            format!("reviewer requested unsupported tool {}", reviewer_call.name),
                        )
                    })?;
                if !definition
                    .capabilities
                    .iter()
                    .any(|capability| capability == "review-tool")
                {
                    return Err(RuntimeError::new(
                        "guardian_review_tool_not_allowed",
                        RuntimeErrorCategory::Provider,
                        format!("reviewer requested non-review tool {}", reviewer_call.name),
                    ));
                }
                let kind = file_tool_kind(definition, &reviewer_call).ok_or_else(|| {
                    RuntimeError::new(
                        "guardian_review_tool_not_allowed",
                        RuntimeErrorCategory::Provider,
                        format!("reviewer requested unsupported tool {}", reviewer_call.name),
                    )
                })?;
                let result = self.execute_file_tool(turn, &reviewer_call, kind);
                if result.status != ToolResultStatus::Ok {
                    return Err(RuntimeError::new(
                        "guardian_review_tool_failed",
                        RuntimeErrorCategory::Provider,
                        result
                            .error
                            .clone()
                            .unwrap_or_else(|| "reviewer helper tool failed".to_string()),
                    ));
                }
                provider.observe_tool_result(&reviewer_call, &result)?;
            }
        }
        Err(RuntimeError::new(
            "guardian_review_no_decision",
            RuntimeErrorCategory::Provider,
            "reviewer did not return a valid decision within the continuation limit",
        ))
    }

    fn guardian_auto_review_tool_call(
        &self,
        turn: &Turn,
        registry: &ToolRegistry,
        call: &ToolCall,
    ) -> Option<GuardianReviewResult> {
        let definition = lookup_tool(registry, call)?;
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let Ok(Some((fallback, request))) =
            guardian_review_request_for_tool(definition, call, &cwd)
        else {
            return None;
        };
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| self.sandbox_policy_for_turn(turn, &cwd, fallback));
        if policy.permission_profile.mode != PermissionMode::AutoReview {
            return None;
        }
        if call.arguments.to_string().len() > 32_000 {
            return Some(guardian_review_result(
                GuardianReviewDecisionKind::Deny,
                RiskLevel::Critical,
                "auto-review input exceeded bounded argument limit",
                call,
            ));
        }
        if value_contains_raw_secret(&call.arguments) {
            return Some(guardian_review_result(
                GuardianReviewDecisionKind::Deny,
                RiskLevel::High,
                "auto-review denied raw secret-bearing tool arguments",
                call,
            ));
        }
        match evaluate_exec(&policy, &request) {
            PolicyDecision::Deny { reason } => Some(guardian_review_result(
                GuardianReviewDecisionKind::Deny,
                RiskLevel::High,
                reason,
                call,
            )),
            PolicyDecision::Ask { risk, reason } if reason.contains("protected path") => {
                Some(guardian_review_result(
                    GuardianReviewDecisionKind::Deny,
                    risk.max(RiskLevel::High),
                    reason,
                    call,
                ))
            }
            PolicyDecision::Ask { risk, reason } => Some(guardian_review_result(
                GuardianReviewDecisionKind::Allow,
                risk,
                format!("local auto-review preflight requires reviewer decision: {reason}"),
                call,
            )),
            PolicyDecision::Allow => Some(guardian_review_result(
                GuardianReviewDecisionKind::Allow,
                RiskLevel::Medium,
                "local auto-review preflight requires reviewer decision for approval-gated tool",
                call,
            )),
        }
    }

    fn emit_guardian_review_diagnostic(
        &mut self,
        turn: &Turn,
        review: &GuardianReviewResult,
        events: &mut Vec<Event>,
    ) {
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::Diagnostic {
                diagnostic: Diagnostic {
                    level: match review.decision {
                        GuardianReviewDecisionKind::Allow => DiagnosticLevel::Info,
                        GuardianReviewDecisionKind::Deny => DiagnosticLevel::Warning,
                    },
                    message: "guardian auto-review decision recorded".to_string(),
                    metadata: BTreeMap::from([
                        ("component".to_string(), "guardian-auto-review".to_string()),
                        (
                            "decision".to_string(),
                            guardian_decision_name(review.decision).to_string(),
                        ),
                        ("risk".to_string(), risk_level_name(review.risk).to_string()),
                        ("reason".to_string(), review.reason.clone()),
                        ("strictJson".to_string(), review.strict_json.clone()),
                    ]),
                },
            },
        ));
    }

    fn execute_shell_task_tool(&mut self, call: &ToolCall) -> ToolResult {
        self.refresh_shell_tasks();
        let action = call
            .arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("list");
        let output = (|| -> Result<String, String> {
            match action {
                "list" => {
                    if self.shell_tasks.is_empty() {
                        Ok(
                            "no background shell tasks (background shell tasks are process-local)"
                                .to_string(),
                        )
                    } else {
                        Ok(self
                            .shell_tasks
                            .values()
                            .map(shell_task_line)
                            .collect::<Vec<_>>()
                            .join("\n"))
                    }
                }
                "read" => {
                    let task_id =
                        required_string_arg(call, "taskId").map_err(|(_, error)| error)?;
                    let max_bytes = usize_arg(call, "maxBytes").unwrap_or(30_000);
                    let read = self
                        .read_background_task(BackgroundReadParams {
                            task_id: task_id.to_string(),
                            max_bytes: Some(max_bytes),
                        })
                        .map_err(|error| error.message)?;
                    let status = format!("{:?}", read.task.status).to_lowercase();
                    let prefix = if read.truncated.unwrap_or(false) {
                        format!(
                            "background shell task {task_id} [{status}] tail ({} of {}B):",
                            max_bytes,
                            read.output_bytes.unwrap_or_default()
                        )
                    } else {
                        format!("background shell task {task_id} [{status}] output:")
                    };
                    if read.output.is_empty() {
                        Ok(format!(
                            "background shell task {task_id} [{status}]: no output yet"
                        ))
                    } else {
                        Ok(format!("{prefix}\n{}", read.output))
                    }
                }
                "kill" => {
                    let task_id =
                        required_string_arg(call, "taskId").map_err(|(_, error)| error)?;
                    self.kill_background_task(BackgroundKillParams {
                        task_id: task_id.to_string(),
                    })
                    .map(|result| result.message)
                    .map_err(|error| error.message)
                }
                other => Err(format!(
                    "shell_task action must be list, read, or kill; got {other}"
                )),
            }
        })();
        match output {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error),
            },
        }
    }

    pub fn list_background_tasks(&mut self) -> BackgroundListResult {
        self.refresh_shell_tasks();
        let mut items = self
            .shell_tasks
            .values()
            .map(background_task_info)
            .collect::<Vec<_>>();
        items.sort_by(|a, b| {
            b.started_at_ms
                .cmp(&a.started_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        BackgroundListResult { items }
    }

    pub fn read_background_task(
        &mut self,
        params: BackgroundReadParams,
    ) -> Result<BackgroundReadResult, RuntimeError> {
        self.refresh_shell_tasks();
        let task = self
            .shell_tasks
            .get(&params.task_id)
            .ok_or_else(|| not_found("background_task", &params.task_id))?;
        let max_bytes = params.max_bytes.unwrap_or(30_000);
        let info = background_task_info(task);
        let output = read_task_tail(&task.output_path, max_bytes);
        let truncated = info
            .output_bytes
            .map(|bytes| bytes > max_bytes as u64)
            .filter(|value| *value);
        Ok(BackgroundReadResult {
            output,
            output_bytes: info.output_bytes,
            max_bytes: Some(max_bytes),
            truncated,
            task: info,
        })
    }

    pub fn kill_background_task(
        &mut self,
        params: BackgroundKillParams,
    ) -> Result<BackgroundKillResult, RuntimeError> {
        self.refresh_shell_tasks();
        let task = self
            .shell_tasks
            .get_mut(&params.task_id)
            .ok_or_else(|| not_found("background_task", &params.task_id))?;
        let message = if task.status == ShellTaskStatus::Running {
            if let Some(child) = task.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            task.child = None;
            task.status = ShellTaskStatus::Killed;
            task.finished_at_ms = Some(now_millis());
            task.exit_code = None;
            format!("killed background shell task {}", params.task_id)
        } else {
            format!(
                "background shell task {} is already {}",
                params.task_id,
                task.status.as_str()
            )
        };
        Ok(BackgroundKillResult {
            task: background_task_info(task),
            message,
        })
    }

    fn refresh_shell_tasks(&mut self) {
        for task in self.shell_tasks.values_mut() {
            if task.status != ShellTaskStatus::Running {
                continue;
            }
            if let Some(child) = task.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        task.status = if status.success() {
                            ShellTaskStatus::Completed
                        } else {
                            ShellTaskStatus::Failed
                        };
                        task.exit_code = status.code();
                        task.finished_at_ms = Some(now_millis());
                        task.child = None;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        task.status = ShellTaskStatus::Failed;
                        task.finished_at_ms = Some(now_millis());
                        task.child = None;
                    }
                }
            }
        }
    }

    fn start_background_shell_task(
        &mut self,
        call: &ToolCall,
        policy: SandboxPolicy,
        request: SandboxExecRequest,
        cwd: String,
        preference: SandboxPreference,
    ) -> ToolResult {
        if let Some(reason) = protected_path_preflight_denial(&policy, &request) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            };
        }
        let task_id = format!(
            "shell-{}-{}",
            self.next_event + 1,
            self.shell_tasks.len() + 1
        );
        let output_dir = Path::new(&cwd).join("output").join("shelltool");
        if let Err(error) = fs::create_dir_all(&output_dir) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(format!("failed to create shell output directory: {error}")),
            };
        }
        let output_path = output_dir.join(format!("{task_id}.log"));
        let stdout = match fs::File::create(&output_path) {
            Ok(file) => file,
            Err(error) => {
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: None,
                    error: Some(format!("failed to create shell output file: {error}")),
                };
            }
        };
        let stderr = match stdout.try_clone() {
            Ok(file) => file,
            Err(error) => {
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: None,
                    error: Some(format!("failed to clone shell output file: {error}")),
                };
            }
        };
        let spawn = spawn_sandboxed_background(SandboxBackgroundSpawnParams {
            policy: policy.clone(),
            request: request.clone(),
            preference,
            approval_granted: true,
            stdout,
            stderr,
            managed_network: None,
        });
        match spawn {
            Ok((child, plan)) => {
                self.shell_tasks.insert(
                    task_id.clone(),
                    ShellTaskRecord {
                        id: task_id.clone(),
                        command: request.command,
                        cwd,
                        output_path: output_path.clone(),
                        status: ShellTaskStatus::Running,
                        child: Some(child),
                        started_at_ms: now_millis(),
                        finished_at_ms: None,
                        exit_code: None,
                    },
                );
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(format!(
                        "background shell task started: {task_id}\noutput: {}\nsandbox: {:?}/{:?}",
                        output_path.display(),
                        plan.enforcement,
                        plan.sandbox_type
                    )),
                    error: None,
                }
            }
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(format!(
                    "background shell task was not started because sandboxed background execution is unavailable: {error}"
                )),
            },
        }
    }

    fn execute_shell_tool(&mut self, turn: &Turn, call: &ToolCall) -> ToolResult {
        let Some(command) = call
            .arguments
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some("shell tool requires a string command argument".to_string()),
            };
        };
        let cwd = call
            .arguments
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                self.threads
                    .get(&turn.thread_id)
                    .map(|thread| thread.project.cwd.clone())
            })
            .unwrap_or_else(|| ".".to_string());
        let request = SandboxExecRequest {
            command,
            cwd: cwd.clone(),
            writes_files: call
                .arguments
                .get("writesFiles")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            uses_network: call
                .arguments
                .get("usesNetwork")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            touches_protected_path: call
                .arguments
                .get("touchesProtectedPath")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            touched_paths: call
                .arguments
                .get("touchedPaths")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        };
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| {
                self.sandbox_policy_for_turn(turn, &cwd, default_shell_policy(&cwd))
            });
        let run_in_background = call
            .arguments
            .get("runInBackground")
            .or_else(|| call.arguments.get("run_in_background"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let sandbox_preference = shell_sandbox_preference_from_call(
            call,
            if run_in_background {
                SandboxPreference::Require
            } else {
                SandboxPreference::Auto
            },
        );
        if let Some(reason) = protected_path_preflight_denial(&policy, &request) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            };
        }
        if run_in_background {
            return self.start_background_shell_task(
                call,
                policy,
                request,
                cwd,
                sandbox_preference,
            );
        }
        let result = execute_sandboxed(SandboxExecParams {
            policy,
            request,
            preference: sandbox_preference,
            approval_granted: call
                .arguments
                .get("approvalGranted")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            managed_network: call
                .arguments
                .get("managedNetwork")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            timeout_ms: call
                .arguments
                .get("timeoutMs")
                .and_then(serde_json::Value::as_u64),
            max_output_bytes: call
                .arguments
                .get("maxOutputBytes")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
        });
        let output = [result.stdout.trim_end(), result.stderr.trim_end()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        match result.decision {
            SandboxPolicyDecision::Allow if result.timed_out => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Aborted,
                output: if output.is_empty() {
                    None
                } else {
                    Some(output)
                },
                error: Some("sandboxed command timed out".to_string()),
            },
            SandboxPolicyDecision::Allow if result.exit_code == Some(0) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            SandboxPolicyDecision::Allow => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: if output.is_empty() {
                    None
                } else {
                    Some(output)
                },
                error: Some(format!(
                    "sandboxed command exited with {:?}",
                    result.exit_code
                )),
            },
            SandboxPolicyDecision::Deny { reason } => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            },
            SandboxPolicyDecision::Ask { reason, .. } => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            },
        }
    }

    fn execute_feedback_tool(&self, turn: &Turn, call: &ToolCall) -> ToolResult {
        match self.execute_feedback_tool_inner(turn, call) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_feedback_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
    ) -> Result<String, (ToolResultStatus, String)> {
        let kind = call
            .arguments
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    "oppi_feedback_submit requires type bug-report or feature-request".to_string(),
                )
            })?;
        if kind != "bug-report" && kind != "feature-request" {
            return Err((
                ToolResultStatus::Error,
                "type must be bug-report or feature-request".to_string(),
            ));
        }
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let (title, body, repo) = feedback_body(kind, call, &cwd)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let draft_path =
            resolve_project_path(&cwd, &format!(".oppi/feedback-drafts/{stamp}-{kind}.md"))?;
        self.preflight_file_access(turn, call, &cwd, &draft_path, true)?;
        if let Some(parent) = draft_path.parent() {
            fs::create_dir_all(parent).map_err(file_io_error)?;
        }
        fs::write(&draft_path, format!("# {title}\n\n{body}")).map_err(file_io_error)?;

        let endpoint = std::env::var("OPPI_FEEDBACK_ENDPOINT")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        let disabled = std::env::var("OPPI_FEEDBACK_DISABLED")
            .ok()
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on" | "yes"));
        if disabled || endpoint.is_none() {
            return Ok(format!(
                "Feedback draft written to {} for repo {repo}. Set OPPI_FEEDBACK_ENDPOINT to enable configured intake submission.",
                draft_path.display()
            ));
        }
        let policy = self.sandbox_policy_for_turn(
            turn,
            &cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::ReadOnly,
                readable_roots: vec![cwd.clone()],
                writable_roots: Vec::new(),
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        if policy.network != NetworkPolicy::Enabled {
            return Ok(format!(
                "Feedback draft written to {}. Intake submission skipped because turn network policy is not enabled.",
                draft_path.display()
            ));
        }
        let endpoint = endpoint.unwrap();
        let url = format!("{endpoint}/v1/intake/{kind}");
        let request = ureq::post(&url).set("content-type", "application/json");
        let request = if let Ok(token) = std::env::var("OPPI_FEEDBACK_TOKEN") {
            if token.trim().is_empty() {
                request
            } else {
                request.set("x-oppi-intake-token", token.trim())
            }
        } else {
            request
        };
        match request.send_json(json!({
            "repo": repo,
            "type": kind,
            "title": title,
            "body": body,
            "labels": if kind == "bug-report" { vec!["bug"] } else { vec!["enhancement"] },
        })) {
            Ok(response) => {
                let value = response.into_json::<Value>().unwrap_or_else(|_| json!({}));
                let issue_url = value
                    .get("issueUrl")
                    .or_else(|| value.get("html_url"))
                    .and_then(Value::as_str)
                    .unwrap_or("submitted");
                Ok(format!(
                    "Feedback submitted: {issue_url}\nDraft copy: {}",
                    draft_path.display()
                ))
            }
            Err(error) => Ok(format!(
                "Feedback draft written to {}. Intake submission failed: {}",
                draft_path.display(),
                feedback_sanitize(&error.to_string())
            )),
        }
    }

    fn execute_render_mermaid_tool(&self, turn: &Turn, call: &ToolCall) -> ToolResult {
        match self.execute_render_mermaid_tool_inner(turn, call) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_image_gen_tool(
        &self,
        turn: &Turn,
        call: &ToolCall,
        parent_provider_config: Option<&DirectModelProviderConfig>,
    ) -> ToolResult {
        match self.execute_image_gen_tool_inner(turn, call, parent_provider_config) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_image_gen_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
        parent_provider_config: Option<&DirectModelProviderConfig>,
    ) -> Result<String, (ToolResultStatus, String)> {
        let prompt = call
            .arguments
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    "image_gen requires a non-empty prompt".to_string(),
                )
            })?;
        let model = call
            .arguments
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("gpt-image-2")
            .trim();
        let output_format = image_output_format(call)?;
        let background = call
            .arguments
            .get("background")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let n = image_generation_count(call)?;
        let source_images = image_source_arguments(call);
        let mask = image_mask_argument(call);
        let backend = selected_image_backend(call, parent_provider_config, n, mask.is_some())?;
        match backend {
            ImageGenBackend::ImageApi => {
                if background == Some("transparent") {
                    let confirmed = call
                        .arguments
                        .get("transparentBackgroundConfirmed")
                        .or_else(|| call.arguments.get("confirmTransparentBackground"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if !confirmed || model != "gpt-image-1.5" {
                        return Err((
                            ToolResultStatus::Error,
                            "image_gen transparent background requires explicit confirmation and model gpt-image-1.5; gpt-image-2 does not support true transparent output".to_string(),
                        ));
                    }
                }
                validate_image_api_options(call, model, &output_format, background, n)?;
            }
            ImageGenBackend::CodexNative => {
                validate_codex_native_image_options(n, mask.as_deref())?;
            }
        }
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let output_path = call
            .arguments
            .get("outputPath")
            .or_else(|| call.arguments.get("output_path"))
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .unwrap_or_else(|| default_image_output_path(prompt, &output_format));
        let paths = image_output_paths(&cwd, &output_format, &output_path, n)?;
        for path in &paths {
            self.preflight_file_access(turn, call, &cwd, path, true)?;
        }
        let overwrite = call
            .arguments
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        for path in &paths {
            if path.exists() && !overwrite {
                return Err((
                    ToolResultStatus::Error,
                    format!(
                        "image_gen output already exists: {}. Set overwrite=true or choose another outputPath.",
                        path.display()
                    ),
                ));
            }
        }
        let image_api_key = if backend == ImageGenBackend::ImageApi {
            let explicit_env = call
                .arguments
                .get("apiKeyEnv")
                .or_else(|| call.arguments.get("api_key_env"))
                .and_then(Value::as_str);
            let candidates = image_api_key_env_candidates(explicit_env).map_err(|error| {
                (
                    ToolResultStatus::Denied,
                    format!("image_gen credential policy denied: {}", error.message),
                )
            })?;
            Some(
                candidates
                    .iter()
                    .find_map(|name| {
                        std::env::var(name)
                            .ok()
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty())
                    })
                    .ok_or_else(|| {
                        (
                            ToolResultStatus::Error,
                            format!(
                                "image_gen requires an approved image backend API key env: {}",
                                candidates.join(", ")
                            ),
                        )
                    })?,
            )
        } else {
            None
        };
        let policy = self.sandbox_policy_for_turn(
            turn,
            &cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::ReadOnly,
                readable_roots: vec![cwd.clone()],
                writable_roots: Vec::new(),
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        if policy.network != NetworkPolicy::Enabled {
            return Err((
                ToolResultStatus::Denied,
                "image_gen requires network policy enabled for the native image backend"
                    .to_string(),
            ));
        }
        if mask.is_some() && source_images.is_empty() {
            return Err((
                ToolResultStatus::Error,
                "image_gen mask requires at least one source image".to_string(),
            ));
        }
        if source_images.len() > 16 {
            return Err((
                ToolResultStatus::Error,
                "image_gen edits support at most 16 source images".to_string(),
            ));
        }
        let mut source_files = Vec::new();
        for source in &source_images {
            let path = image_input_path(&cwd, source)?;
            self.preflight_file_access(turn, call, &cwd, &path, false)?;
            let metadata = fs::metadata(&path).map_err(file_io_error)?;
            if !metadata.is_file() {
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen source image is not a file: {}", path.display()),
                ));
            }
            if metadata.len() > IMAGE_GEN_MAX_INPUT_BYTES {
                return Err((
                    ToolResultStatus::Error,
                    format!(
                        "image_gen source image exceeds 50MB limit: {}",
                        path.display()
                    ),
                ));
            }
            let bytes = fs::read(&path).map_err(file_io_error)?;
            source_files.push((source.clone(), path, bytes));
        }
        let mask_file = if let Some(mask) = &mask {
            let path = image_input_path(&cwd, mask)?;
            self.preflight_file_access(turn, call, &cwd, &path, false)?;
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !extension.eq_ignore_ascii_case("png") {
                return Err((
                    ToolResultStatus::Error,
                    "image_gen mask must be a PNG file with an alpha channel".to_string(),
                ));
            }
            let metadata = fs::metadata(&path).map_err(file_io_error)?;
            if !metadata.is_file() {
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen mask is not a file: {}", path.display()),
                ));
            }
            if metadata.len() > IMAGE_GEN_MAX_INPUT_BYTES {
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen mask exceeds 50MB limit: {}", path.display()),
                ));
            }
            let bytes = fs::read(&path).map_err(file_io_error)?;
            Some((mask.clone(), path, bytes))
        } else {
            None
        };
        let started = Instant::now();
        let (images, backend_label, backend_model, endpoint) = match backend {
            ImageGenBackend::CodexNative => {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(120))
                    .build();
                let auth = read_or_refresh_codex_auth(&agent).map_err(|error| {
                    (
                        ToolResultStatus::Error,
                        format!(
                            "image_gen Codex native image_generation auth failed: {}",
                            error.message
                        ),
                    )
                })?;
                let endpoint = openai_codex_responses_endpoint(parent_provider_config);
                let codex_model = codex_native_image_model(parent_provider_config);
                let body = codex_native_image_request_body(
                    &codex_model,
                    prompt,
                    &output_format,
                    &source_files,
                );
                let response = agent
                    .post(&endpoint)
                    .set("authorization", &format!("Bearer {}", auth.access_token))
                    .set("chatgpt-account-id", &auth.account_id)
                    .set("originator", "pi")
                    .set("User-Agent", &codex_user_agent())
                    .set("OpenAI-Beta", "responses=experimental")
                    .set("accept", "text/event-stream")
                    .set("content-type", "application/json")
                    .send_json(body);
                let images = match response {
                    Ok(response) => {
                        parse_codex_image_generation_stream(BufReader::new(response.into_reader()))?
                    }
                    Err(ureq::Error::Status(status, response)) => {
                        let detail = response.into_string().unwrap_or_default();
                        return Err((
                            ToolResultStatus::Error,
                            format!(
                                "image_gen Codex native image_generation HTTP request failed with status {status}{}",
                                if detail.trim().is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", compact_provider_error(&detail))
                                }
                            ),
                        ));
                    }
                    Err(error) => {
                        return Err((
                            ToolResultStatus::Error,
                            format!(
                                "image_gen Codex native image_generation transport error: {error}"
                            ),
                        ));
                    }
                };
                (images, "codex-native".to_string(), codex_model, endpoint)
            }
            ImageGenBackend::ImageApi => {
                let api_key = image_api_key.ok_or_else(|| {
                    (
                        ToolResultStatus::Error,
                        "image_gen internal backend credential selection failed".to_string(),
                    )
                })?;
                let edits = !source_files.is_empty();
                let endpoint = if edits {
                    image_api_endpoint(call, "images/edits")
                } else {
                    image_generation_endpoint(call)
                };
                let generation_body = image_generation_request_body(
                    call,
                    model,
                    prompt,
                    &output_format,
                    background,
                    n,
                );
                let response = if edits {
                    let boundary = format!(
                        "oppi-image-{}-{}",
                        std::process::id(),
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    let mut body = Vec::new();
                    let n_value = n.to_string();
                    for (key, value) in [
                        ("model", Some(model)),
                        ("prompt", Some(prompt)),
                        ("n", Some(n_value.as_str())),
                        ("output_format", Some(output_format.as_str())),
                        ("background", background),
                        ("size", image_optional_string_arg(call, &["size"])),
                        ("quality", image_optional_string_arg(call, &["quality"])),
                        (
                            "moderation",
                            image_optional_string_arg(call, &["moderation"]),
                        ),
                        (
                            "input_fidelity",
                            image_optional_string_arg(call, &["inputFidelity", "input_fidelity"]),
                        ),
                    ] {
                        if let Some(value) = value {
                            multipart_push_text(&mut body, &boundary, key, value);
                        }
                    }
                    if let Some(value) =
                        image_optional_u64_arg(call, &["outputCompression", "output_compression"])
                    {
                        multipart_push_text(
                            &mut body,
                            &boundary,
                            "output_compression",
                            &value.to_string(),
                        );
                    }
                    for (_, path, bytes) in &source_files {
                        let filename = path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("image.png");
                        let field = if source_files.len() == 1 {
                            "image"
                        } else {
                            "image[]"
                        };
                        multipart_push_file(
                            &mut body,
                            &boundary,
                            field,
                            filename,
                            image_mime_type_for_path(path),
                            bytes,
                        );
                    }
                    if let Some((_, path, bytes)) = &mask_file {
                        let filename = path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("mask.png");
                        multipart_push_file(
                            &mut body,
                            &boundary,
                            "mask",
                            filename,
                            "image/png",
                            bytes,
                        );
                    }
                    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
                    ureq::post(&endpoint)
                        .set("authorization", &format!("Bearer {api_key}"))
                        .set(
                            "content-type",
                            &format!("multipart/form-data; boundary={boundary}"),
                        )
                        .send_bytes(&body)
                } else {
                    ureq::post(&endpoint)
                        .set("authorization", &format!("Bearer {api_key}"))
                        .set("content-type", "application/json")
                        .send_json(generation_body)
                };
                let value = match response {
                    Ok(response) => response.into_json::<Value>().map_err(|error| {
                        (
                            ToolResultStatus::Error,
                            format!("image_gen backend returned invalid JSON: {error}"),
                        )
                    })?,
                    Err(ureq::Error::Status(status, response)) => {
                        let detail = response.into_string().unwrap_or_default();
                        return Err((
                            ToolResultStatus::Error,
                            format!(
                                "image_gen backend HTTP request failed with status {status}{}",
                                if detail.trim().is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", compact_provider_error(&detail))
                                }
                            ),
                        ));
                    }
                    Err(error) => {
                        return Err((
                            ToolResultStatus::Error,
                            format!("image_gen backend transport error: {error}"),
                        ));
                    }
                };
                let images = value
                    .get("data")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                    .cloned()
                    .ok_or_else(|| {
                        (
                            ToolResultStatus::Error,
                            "image_gen backend response did not include data[0]".to_string(),
                        )
                    })?;
                (
                    images,
                    "openai-images".to_string(),
                    model.to_string(),
                    endpoint,
                )
            }
        };
        let revised_prompt = images
            .iter()
            .find_map(|image| {
                image
                    .get("revised_prompt")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })
            .map(str::to_string);
        let mut written = Vec::new();
        for (index, image) in images.iter().enumerate() {
            let Some(path) = paths.get(index) else {
                break;
            };
            let bytes = if let Some(b64) = image.get("b64_json").and_then(Value::as_str) {
                decode_base64(b64).map_err(|error| (ToolResultStatus::Error, error))?
            } else if let Some(url) = image.get("url").and_then(Value::as_str) {
                download_image_url(url)?
            } else {
                return Err((
                    ToolResultStatus::Error,
                    "image_gen backend response requires data[0].b64_json or data[0].url"
                        .to_string(),
                ));
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(file_io_error)?;
            }
            fs::write(path, &bytes).map_err(file_io_error)?;
            let (width, height) = image_dimensions(&bytes, &output_format);
            written.push((path.clone(), bytes.len(), width, height));
        }
        if written.is_empty() {
            return Err((
                ToolResultStatus::Error,
                "image_gen backend response did not include writable image data".to_string(),
            ));
        }
        let output_paths = written
            .iter()
            .map(|(path, _, _, _)| path.display().to_string())
            .collect::<Vec<_>>();
        let images = written
            .iter()
            .zip(images.iter())
            .map(|((path, bytes, width, height), source)| {
                let mut image = json!({
                    "outputPath": path.display().to_string(),
                    "bytes": bytes,
                });
                if let Some(width) = width {
                    image["width"] = json!(width);
                }
                if let Some(height) = height {
                    image["height"] = json!(height);
                }
                if let Some(revised) = source.get("revised_prompt").and_then(Value::as_str) {
                    image["revisedPrompt"] = json!(revised);
                }
                image
            })
            .collect::<Vec<_>>();
        let (_, first_bytes, width, height) = &written[0];
        let mut output = json!({
            "message": format!("Generated {} image(s) with native {backend_label} image backend", written.len()),
            "outputPath": output_paths[0],
            "outputPaths": output_paths,
            "mimeType": mime_type_for_image_format(&output_format),
            "bytes": first_bytes,
            "images": images,
            "backend": backend_label,
            "model": backend_model,
            "endpoint": redacted_endpoint(&endpoint),
            "durationMs": started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        });
        if let Some(revised_prompt) = revised_prompt {
            output["revisedPrompt"] = json!(revised_prompt);
        }
        if let Some(width) = width {
            output["width"] = json!(width);
        }
        if let Some(height) = height {
            output["height"] = json!(height);
        }
        Ok(output.to_string())
    }

    fn execute_render_mermaid_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
    ) -> Result<String, (ToolResultStatus, String)> {
        let source = required_string_arg(call, "mermaid")?
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .to_string();
        if source.is_empty() {
            return Err((
                ToolResultStatus::Error,
                "mermaid must not be empty".to_string(),
            ));
        }
        if source.len() > 20_000 {
            return Err((
                ToolResultStatus::Error,
                format!(
                    "Mermaid source is too large ({} chars; max 20000)",
                    source.len()
                ),
            ));
        }
        let ascii = render_mermaid_fallback(&source)?;
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let saved = if let Some(output_path) =
            call.arguments.get("outputPath").and_then(Value::as_str)
        {
            let path = resolve_project_path(&cwd, output_path)?;
            self.preflight_file_access(turn, call, &cwd, &path, true)?;
            let overwrite = call
                .arguments
                .get("overwrite")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if path.exists() && !overwrite {
                return Err((
                    ToolResultStatus::Error,
                    format!(
                        "Output already exists: {}. Set overwrite=true or choose another outputPath.",
                        path.display()
                    ),
                ));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(file_io_error)?;
            }
            fs::write(&path, &ascii).map_err(file_io_error)?;
            Some(path.display().to_string())
        } else {
            None
        };
        let mut lines = vec!["Rendered Mermaid diagram with Rust fallback.".to_string()];
        if let Some(path) = saved {
            lines.push(format!("Saved ASCII output to {path}."));
        }
        lines.push(String::new());
        lines.push("```text".to_string());
        lines.push(ascii);
        lines.push("```".to_string());
        Ok(lines.join("\n"))
    }

    fn execute_file_tool(&self, turn: &Turn, call: &ToolCall, kind: FileToolKind) -> ToolResult {
        match self.execute_file_tool_inner(turn, call, kind) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_file_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
        kind: FileToolKind,
    ) -> Result<String, (ToolResultStatus, String)> {
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        match kind {
            FileToolKind::Read => {
                let path = required_string_arg(call, "path")?;
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, false)?;
                let max_bytes = usize_arg(call, "maxBytes").unwrap_or(65_536);
                if call.arguments.get("offset").is_some() || call.arguments.get("limit").is_some() {
                    let offset = usize_arg(call, "offset").unwrap_or(1).max(1);
                    let limit = usize_arg(call, "limit").unwrap_or(200).max(1);
                    let text = fs::read_to_string(&path).map_err(file_io_error)?;
                    return Ok(text
                        .lines()
                        .skip(offset.saturating_sub(1))
                        .take(limit)
                        .collect::<Vec<_>>()
                        .join("\n"));
                }
                let bytes = fs::read(&path).map_err(file_io_error)?;
                Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]).to_string())
            }
            FileToolKind::List => {
                let root = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let root = resolve_project_path(&cwd, root)?;
                self.preflight_file_access(turn, call, &cwd, &root, false)?;
                let max_results = usize_arg(call, "maxResults")
                    .or_else(|| usize_arg(call, "limit"))
                    .unwrap_or(200)
                    .max(1);
                let mut entries = fs::read_dir(&root)
                    .map_err(file_io_error)?
                    .filter_map(Result::ok)
                    .map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let suffix = if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                        {
                            "/"
                        } else {
                            ""
                        };
                        format!("{name}{suffix}")
                    })
                    .collect::<Vec<_>>();
                entries.sort();
                entries.truncate(max_results);
                if entries.is_empty() {
                    Ok("empty directory".to_string())
                } else {
                    Ok(entries.join("\n"))
                }
            }
            FileToolKind::Write => {
                let path = required_string_arg(call, "path")?;
                let content = required_string_arg(call, "content")?;
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, true)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(file_io_error)?;
                }
                fs::write(&path, content).map_err(file_io_error)?;
                Ok(format!(
                    "wrote {} bytes to {}",
                    content.len(),
                    path.display()
                ))
            }
            FileToolKind::Edit => {
                let path = required_string_arg(call, "path")?;
                let old_text = required_string_arg(call, "oldText")?;
                let new_text = required_string_arg(call, "newText")?;
                let replace_all = call
                    .arguments
                    .get("replaceAll")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, true)?;
                let text = fs::read_to_string(&path).map_err(file_io_error)?;
                let matches = text.matches(old_text).count();
                if matches == 0 {
                    return Err((ToolResultStatus::Error, "oldText was not found".to_string()));
                }
                if matches > 1 && !replace_all {
                    return Err((
                        ToolResultStatus::Error,
                        format!("oldText matched {matches} times; set replaceAll to true"),
                    ));
                }
                let updated = if replace_all {
                    text.replace(old_text, new_text)
                } else {
                    text.replacen(old_text, new_text, 1)
                };
                fs::write(&path, updated).map_err(file_io_error)?;
                Ok(format!(
                    "edited {} replacement(s) in {}",
                    matches,
                    path.display()
                ))
            }
            FileToolKind::Search => {
                let query =
                    first_string_arg(&call.arguments, &["query", "pattern"]).ok_or_else(|| {
                        (
                            ToolResultStatus::Error,
                            "tool argument 'query' or 'pattern' must be a non-empty string"
                                .to_string(),
                        )
                    })?;
                let root = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let root = resolve_project_path(&cwd, root)?;
                self.preflight_file_access(turn, call, &cwd, &root, false)?;
                let max_results = usize_arg(call, "maxResults").unwrap_or(50);
                let max_bytes_per_file = usize_arg(call, "maxBytesPerFile").unwrap_or(1_048_576);
                let mut results = Vec::new();
                search_files_under(
                    self,
                    turn,
                    call,
                    &cwd,
                    &root,
                    &query,
                    max_results,
                    max_bytes_per_file,
                    &mut results,
                );
                if results.is_empty() {
                    Ok("no matches".to_string())
                } else {
                    Ok(results.join("\n"))
                }
            }
        }
    }

    fn preflight_file_access(
        &self,
        turn: &Turn,
        call: &ToolCall,
        cwd: &str,
        path: &Path,
        writes: bool,
    ) -> Result<(), (ToolResultStatus, String)> {
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| {
                self.sandbox_policy_for_turn(turn, cwd, default_file_tool_policy(cwd, writes))
            });
        if !file_path_allowed_by_roots(path, cwd, &policy, writes) {
            return Err((
                ToolResultStatus::Denied,
                "filesystem policy blocks access outside configured roots".to_string(),
            ));
        }
        let request = SandboxExecRequest {
            command: format!("file-tool:{}", call.name),
            cwd: cwd.to_string(),
            writes_files: writes,
            uses_network: false,
            touches_protected_path: false,
            touched_paths: vec![path.to_string_lossy().to_string()],
        };
        let approval_granted = call
            .arguments
            .get("approvalGranted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match evaluate_exec(&policy, &request) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Deny { reason } => Err((ToolResultStatus::Denied, reason)),
            PolicyDecision::Ask { reason, .. }
                if approval_granted && !reason.contains("protected path") =>
            {
                Ok(())
            }
            PolicyDecision::Ask { reason, .. } => Err((ToolResultStatus::Denied, reason)),
        }
    }

    fn emit_hook_diagnostic(
        &mut self,
        turn: &Turn,
        hook: &str,
        message: String,
        events: &mut Vec<Event>,
    ) {
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::Diagnostic {
                diagnostic: Diagnostic {
                    level: DiagnosticLevel::Info,
                    message,
                    metadata: BTreeMap::from([("hook".to_string(), hook.to_string())]),
                },
            },
        ));
    }

    fn record_tool_pair(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: SimulatedToolUse,
    ) -> Result<Vec<Event>, RuntimeError> {
        self.record_tool_batch_items(
            thread_id,
            turn_id,
            vec![simulated],
            ToolBatchExecution::Exclusive,
        )
    }

    fn record_tool_batch_items(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        tools: Vec<SimulatedToolUse>,
        execution: ToolBatchExecution,
    ) -> Result<Vec<Event>, RuntimeError> {
        self.validate_tool_batch_items(thread_id, turn_id, &tools)?;

        let mut events = Vec::new();
        match execution {
            ToolBatchExecution::Concurrent => {
                for simulated in &tools {
                    self.emit_tool_call_started(thread_id, turn_id, simulated, &mut events);
                }
                for simulated in tools {
                    self.emit_tool_result_completed(thread_id, turn_id, simulated, &mut events);
                }
            }
            ToolBatchExecution::Exclusive => {
                for simulated in tools {
                    self.emit_tool_call_started(thread_id, turn_id, &simulated, &mut events);
                    self.emit_tool_result_completed(thread_id, turn_id, simulated, &mut events);
                }
            }
        }
        Ok(events)
    }

    fn emit_tool_call_started(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: &SimulatedToolUse,
        events: &mut Vec<Event>,
    ) {
        let call_item = self.item(
            thread_id,
            turn_id,
            ItemKind::ToolCall(simulated.call.clone()),
        );
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ToolCallStarted {
                call: simulated.call.clone(),
            },
        ));
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ItemCompleted { item: call_item },
        ));

        if simulated.require_approval
            && !self.has_approved_tool_call(thread_id, turn_id, &simulated.call.id)
        {
            self.next_approval += 1;
            let request = ApprovalRequest {
                id: format!("approval-{}", self.next_approval),
                reason: format!("tool {} requires approval", simulated.call.name),
                risk: RiskLevel::Medium,
                tool_call: Some(simulated.call.clone()),
            };
            self.approvals.insert(
                request.id.clone(),
                OwnedApprovalRequest {
                    thread_id: thread_id.to_string(),
                    turn_id: Some(turn_id.to_string()),
                    tool_call_id: Some(simulated.call.id.clone()),
                    tool_call: Some(simulated.call.clone()),
                    outcome: None,
                },
            );
            events.push(self.event(
                thread_id,
                Some(turn_id),
                EventKind::ApprovalRequested { request },
            ));
        }
    }

    fn emit_tool_result_completed(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: SimulatedToolUse,
        events: &mut Vec<Event>,
    ) {
        let result_item = self.item(
            thread_id,
            turn_id,
            ItemKind::ToolResult(simulated.result.clone()),
        );
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ToolCallCompleted {
                result: simulated.result.clone(),
            },
        ));
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ItemCompleted { item: result_item },
        ));
    }

    fn set_phase(&mut self, turn: &mut Turn, phase: TurnPhase, emitted: &mut Vec<Event>) {
        turn.phase = phase;
        self.turns.insert(turn.id.clone(), turn.clone());
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnPhaseChanged { phase },
        ));
    }

    fn thread(&self, thread_id: &str) -> Result<&Thread, RuntimeError> {
        self.threads
            .get(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))
    }

    fn turn(&self, turn_id: &str) -> Result<&Turn, RuntimeError> {
        self.turns
            .get(turn_id)
            .ok_or_else(|| not_found("turn", turn_id))
    }

    fn turn_in_thread(&self, thread_id: &str, turn_id: &str) -> Result<&Turn, RuntimeError> {
        let turn = self.turn(turn_id)?;
        if turn.thread_id != thread_id {
            return Err(RuntimeError::new(
                "turn_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("turn {turn_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(turn)
    }

    fn agent_project_root(&self, thread_id: Option<&str>) -> Result<PathBuf, RuntimeError> {
        if let Some(thread_id) = thread_id {
            return Ok(PathBuf::from(&self.thread(thread_id)?.project.cwd));
        }
        Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn agent_writable_directory(
        &self,
        thread_id: &str,
        source: AgentSource,
    ) -> Result<PathBuf, RuntimeError> {
        match source {
            AgentSource::Project => Ok(self
                .agent_project_root(Some(thread_id))?
                .join(".oppi")
                .join("agents")),
            AgentSource::User => {
                self.thread(thread_id)?;
                Ok(default_oppi_agent_dir().join("oppi").join("agents"))
            }
            AgentSource::ClaudeProject
            | AgentSource::ClaudeUser
            | AgentSource::BuiltIn
            | AgentSource::Plugin
            | AgentSource::Cli
            | AgentSource::ManagedPolicy => Err(RuntimeError::new(
                "agent_source_read_only",
                RuntimeErrorCategory::InvalidRequest,
                format!("agent source {source:?} is read-only and not writable"),
            )),
        }
    }

    fn find_agent_definition_for_source(
        &self,
        thread_id: &str,
        source: AgentSource,
        name: &str,
    ) -> Result<Option<AgentDefinition>, RuntimeError> {
        let result = self.list_agents(AgentListParams {
            thread_id: Some(thread_id.to_string()),
        })?;
        for resolved in result.items {
            let mut definitions = vec![resolved.active];
            definitions.extend(resolved.shadowed);
            if let Some(definition) = definitions
                .into_iter()
                .find(|definition| definition.name == name && definition.source == Some(source))
            {
                return Ok(Some(definition));
            }
        }
        Ok(None)
    }

    fn active_agent(
        &self,
        thread_id: Option<&str>,
        name: &str,
    ) -> Result<AgentDefinition, RuntimeError> {
        self.list_agents(AgentListParams {
            thread_id: thread_id.map(str::to_string),
        })?
        .items
        .into_iter()
        .find(|resolved| resolved.active.name == name)
        .map(|resolved| resolved.active)
        .ok_or_else(|| not_found("agent", name))
    }

    fn agent_run(&self, run_id: &str) -> Result<&AgentRun, RuntimeError> {
        self.agent_runs
            .get(run_id)
            .ok_or_else(|| not_found("agent run", run_id))
    }

    fn agent_run_in_thread(
        &self,
        thread_id: &str,
        run_id: &str,
    ) -> Result<&AgentRun, RuntimeError> {
        let run = self.agent_run(run_id)?;
        if run.thread_id != thread_id {
            return Err(RuntimeError::new(
                "agent_run_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("agent run {run_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(run)
    }

    fn approval_in_thread_mut(
        &mut self,
        thread_id: &str,
        approval_id: &str,
    ) -> Result<&mut OwnedApprovalRequest, RuntimeError> {
        let owner = self
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| not_found("approval", approval_id))?;
        if owner.thread_id != thread_id {
            return Err(RuntimeError::new(
                "approval_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("approval {approval_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(owner)
    }

    fn question_in_thread_mut(
        &mut self,
        thread_id: &str,
        question_id: &str,
    ) -> Result<&mut OwnedQuestionRequest, RuntimeError> {
        let owner = self
            .questions
            .get_mut(question_id)
            .ok_or_else(|| not_found("question", question_id))?;
        if owner.thread_id != thread_id {
            return Err(RuntimeError::new(
                "question_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("question {question_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(owner)
    }

    fn has_approved_tool_call(&self, thread_id: &str, turn_id: &str, call_id: &str) -> bool {
        self.approvals.values().any(|owner| {
            owner.thread_id == thread_id
                && owner.tool_call_id.as_deref() == Some(call_id)
                && owner.outcome == Some(ApprovalOutcome::Approved)
                && match owner.turn_id.as_deref() {
                    Some(owner_turn_id) => owner_turn_id == turn_id,
                    None => true,
                }
        })
    }

    fn validate_tool_batch_items(
        &self,
        thread_id: &str,
        turn_id: &str,
        tools: &[SimulatedToolUse],
    ) -> Result<(), RuntimeError> {
        validate_tool_pairing(tools)?;
        for simulated in tools {
            if simulated.require_approval
                && simulated.result.status == ToolResultStatus::Ok
                && !self.has_approved_tool_call(thread_id, turn_id, &simulated.call.id)
            {
                return Err(RuntimeError::new(
                    "tool_requires_approval",
                    RuntimeErrorCategory::ApprovalRequired,
                    format!(
                        "tool call {} requires approval before it can complete successfully",
                        simulated.call.id
                    ),
                ));
            }
        }
        Ok(())
    }

    fn agent_run_mut(&mut self, run_id: &str) -> Result<&mut AgentRun, RuntimeError> {
        self.agent_runs
            .get_mut(run_id)
            .ok_or_else(|| not_found("agent run", run_id))
    }

    fn bump_thread_counter(&mut self, id: &str) {
        self.next_thread = self.next_thread.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_turn_counter(&mut self, id: &str) {
        self.next_turn = self.next_turn.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_item_counter(&mut self, id: &str) {
        self.next_item = self.next_item.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_approval_counter(&mut self, id: &str) {
        self.next_approval = self.next_approval.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_question_counter(&mut self, id: &str) {
        self.next_question = self.next_question.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_agent_run_counter(&mut self, id: &str) {
        self.next_agent_run = self.next_agent_run.max(numeric_suffix(id).unwrap_or(0));
    }

    fn item(&mut self, thread_id: &str, turn_id: &str, kind: ItemKind) -> Item {
        self.next_item += 1;
        Item {
            id: format!("item-{}", self.next_item),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            kind,
        }
    }

    fn event(&mut self, thread_id: &str, turn_id: Option<&str>, kind: EventKind) -> Event {
        self.next_event += 1;
        let event = Event {
            id: self.next_event,
            thread_id: thread_id.to_string(),
            turn_id: turn_id.map(ToString::to_string),
            created_at_ms: now_millis(),
            kind,
        };
        if let EventKind::PermissionReviewStarted { record }
        | EventKind::PermissionReviewCompleted { record } = &event.kind
        {
            self.guardian_remember_review_record(record.clone());
        }
        self.event_store
            .append(event.clone())
            .expect("in-memory event store append should not fail");
        if let Some(mirror) = &self.event_mirror
            && let Ok(mut events) = mirror.lock()
        {
            events.push(event.clone());
        }
        event
    }
}
