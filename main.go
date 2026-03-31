package main

import (
	"fmt"
	"log"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/catalinj/arta/inputpanel"
	"github.com/catalinj/arta/sidebar"
	"github.com/catalinj/arta/workspace"

	"github.com/Gaurav-Gosain/tuios/pkg/tuios"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

const sidebarWidth = 30
const inputPanelHeight = 15
const tmuxPrefix = "arta_"

type focus int

const (
	focusSidebar focus = iota
	focusTerminal
	focusInput
)

// What the input panel is being used for
type inputPurpose int

const (
	purposeNone inputPurpose = iota
	purposeProjectPath
	purposeProjectName
	purposeRenameProject
	purposeRenameSession
	purposeConfirmCloseSession
	purposeConfirmRemoveProject
)

type model struct {
	sidebar    sidebar.Model
	tuios      *tuios.Model
	inputPanel inputpanel.Model
	workspace  *workspace.Workspace
	focus      focus
	width      int
	height     int
	ready      bool

	// Input context
	inputPurpose inputPurpose
	inputContext  string // stores context data
	pendingPath  string

	// Terminal state
	termWindowIdx int
	activeSession string
	prefixActive  bool
}

// Bell check tick message
type bellCheckMsg struct{}

func bellCheckTick() tea.Cmd {
	return tea.Tick(3*time.Second, func(t time.Time) tea.Msg {
		return bellCheckMsg{}
	})
}

// Check tmux for sessions with bell alerts
func checkTmuxBells(activeSession string) []string {
	out, err := exec.Command("tmux", "list-sessions", "-F", "#{session_name} #{session_alerts}").Output()
	if err != nil {
		return nil
	}
	var alertedSessions []string
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		parts := strings.SplitN(line, " ", 2)
		if len(parts) < 2 {
			continue
		}
		name := parts[0]
		alerts := parts[1]
		if !strings.HasPrefix(name, tmuxPrefix) {
			continue
		}
		sessionID := strings.TrimPrefix(name, tmuxPrefix)
		// Only alert for non-active sessions that have alerts
		if sessionID != activeSession && alerts != "" {
			alertedSessions = append(alertedSessions, sessionID)
		}
	}
	return alertedSessions
}

// --- tmux helpers ---

func tmuxSessionExists(name string) bool {
	return exec.Command("tmux", "has-session", "-t", name).Run() == nil
}

func tmuxCreateSession(name, dir string) {
	exec.Command("tmux", "new-session", "-d", "-s", name, "-n", "claude", "-c", dir).Run()
	exec.Command("tmux", "send-keys", "-t", name+":claude", "claude", "Enter").Run()
	exec.Command("tmux", "new-window", "-t", name, "-n", "terminal", "-c", dir).Run()
	exec.Command("tmux", "select-window", "-t", name+":claude").Run()
	exec.Command("tmux", "set-option", "-t", name, "mouse", "on").Run()
	exec.Command("tmux", "set-option", "-t", name, "monitor-activity", "on").Run()
	exec.Command("tmux", "set-option", "-t", name, "activity-action", "any").Run()
	exec.Command("tmux", "set-option", "-t", name, "monitor-bell", "on").Run()
	exec.Command("tmux", "set-option", "-t", name, "bell-action", "any").Run()
}

func tmuxKillSession(name string) {
	exec.Command("tmux", "kill-session", "-t", name).Run()
}

func tmuxKillAllCterm() {
	out, err := exec.Command("tmux", "list-sessions", "-F", "#{session_name}").Output()
	if err != nil {
		return
	}
	for _, line := range strings.Split(string(out), "\n") {
		if strings.HasPrefix(line, tmuxPrefix) {
			tmuxKillSession(line)
		}
	}
}

// --- Model ---

func newModel() model {
	ws := workspace.New()
	ws.Load()

	alive := ws.Sessions[:0]
	for _, s := range ws.Sessions {
		if tmuxSessionExists(tmuxPrefix + s.ID) {
			alive = append(alive, s)
		}
	}
	ws.Sessions = alive
	ws.Save()

	sb := sidebar.New(ws)
	ip := inputpanel.New()

	t := tuios.New(
		tuios.WithTheme("doom-one"),
		tuios.WithAnimations(false),
		tuios.WithWorkspaces(9),
		tuios.WithBorderStyle("hidden"),
		tuios.WithASCIIOnly(false),
		tuios.WithHideWindowButtons(true),
		tuios.WithDockbarPosition("hidden"),
		tuios.WithScrollbackLines(10000),
	)
	t.AutoTiling = true

	return model{
		sidebar:       sb,
		tuios:         t,
		inputPanel:    ip,
		workspace:     ws,
		focus:         focusSidebar,
		termWindowIdx: -1,
	}
}

func (m model) Init() tea.Cmd {
	return tea.Batch(m.tuios.Init(), m.inputPanel.Init(), bellCheckTick())
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.ready = true
		m.sidebar.SetSize(sidebarWidth, m.height)

		tuiosWidth := m.width - sidebarWidth - 1
		if tuiosWidth < 10 {
			tuiosWidth = 10
		}
		tuiosHeight := m.height
		if m.inputPanel.Active() {
			tuiosHeight = m.height - inputPanelHeight
		}
		updated, cmd := m.tuios.Update(tea.WindowSizeMsg{Width: tuiosWidth, Height: tuiosHeight})
		m.tuios = updated.(*tuios.Model)
		cmds = append(cmds, cmd)
		return &m, tea.Batch(cmds...)

	case tea.KeyMsg:
		// Input panel gets priority when active
		if m.inputPanel.Active() {
			var cmd tea.Cmd
			m.inputPanel, cmd = m.inputPanel.Update(msg)
			cmds = append(cmds, cmd)
			return &m, tea.Batch(cmds...)
		}

		// Prefix key system
		if m.prefixActive {
			m.prefixActive = false
			switch msg.String() {
			case "left":
				m.focus = focusSidebar
				m.sidebar.SetFocused(true)
				m.tuios.Mode = 0
				return &m, nil
			case "right":
				if m.termWindowIdx >= 0 {
					m.focus = focusTerminal
					m.sidebar.SetFocused(false)
					m.tuios.Mode = 1
				}
				return &m, nil
			}
		}
		if msg.String() == "ctrl+@" || msg.String() == "ctrl+space" {
			m.prefixActive = true
			return &m, nil
		}

		if m.focus == focusSidebar {
			var cmd tea.Cmd
			m.sidebar, cmd = m.sidebar.Update(msg)
			cmds = append(cmds, cmd)
		} else {
			updated, cmd := m.tuios.Update(msg)
			m.tuios = updated.(*tuios.Model)
			cmds = append(cmds, cmd)
		}
		return &m, tea.Batch(cmds...)

	case tea.MouseMsg:
		if m.inputPanel.Active() {
			return &m, nil
		}
		mouse := msg.Mouse()
		if mouse.X < sidebarWidth {
			m.focus = focusSidebar
			m.sidebar.SetFocused(true)
			var cmd tea.Cmd
			m.sidebar, cmd = m.sidebar.Update(msg)
			cmds = append(cmds, cmd)
		} else if m.termWindowIdx >= 0 {
			m.focus = focusTerminal
			m.sidebar.SetFocused(false)
			m.tuios.Mode = 1
			updated, cmd := m.tuios.Update(msg)
			m.tuios = updated.(*tuios.Model)
			cmds = append(cmds, cmd)
		}
		return &m, tea.Batch(cmds...)

	// --- Input panel results ---

	case inputpanel.SubmitMsg:
		return m.handleInputSubmit(msg.Value)

	case inputpanel.CancelMsg:
		m.inputPurpose = purposeNone
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil

	// --- Sidebar messages ---

	case sidebar.SelectSessionMsg:
		m.sidebar.SetSelected(msg.ID)
		m.sidebar.ClearAttention(msg.ID)
		m.attachToSession(msg.ID)
		m.focus = focusTerminal
		m.sidebar.SetFocused(false)
		m.tuios.Mode = 1
		return &m, nil

	case sidebar.NewSessionMsg:
		return m.createSession(msg.Project)

	case sidebar.CloseSessionMsg:
		m.openInput(purposeConfirmCloseSession, fmt.Sprintf("Close session %s? (y/n)", msg.ID), "", msg.ID)
		return &m, nil

	case sidebar.AddProjectMsg:
		home, _ := os.UserHomeDir()
		m.openInputPath(purposeProjectPath, "Project directory", home+"/")
		return &m, nil

	case sidebar.RemoveProjectMsg:
		m.openInput(purposeConfirmRemoveProject, fmt.Sprintf("Remove %s and all sessions? (y/n)", msg.Name), "", msg.Name)
		return &m, nil

	case sidebar.RenameProjectMsg:
		m.openInput(purposeRenameProject, "Rename project", msg.Old, msg.Old)
		return &m, nil

	case sidebar.RenameSessionMsg:
		m.openInput(purposeRenameSession, "Rename session", msg.ID, msg.ID)
		return &m, nil

	case sidebar.MoveProjectMsg:
		cursorProject := m.sidebar.GetCursorProject()
		if cursorProject != "" {
			for i, p := range m.workspace.Projects {
				if p.Name == cursorProject {
					target := i + msg.Direction
					if target >= 0 && target < len(m.workspace.Projects) {
						m.workspace.SwapProjects(i, target)
					}
					break
				}
			}
		}
		m.sidebar.Refresh()
		return &m, nil

	case sidebar.MoveSessionMsg:
		m.workspace.SwapSessionInProject(msg.ID, msg.Direction)
		m.sidebar.Refresh()
		return &m, nil

	case sidebar.FocusTerminalMsg:
		if m.termWindowIdx >= 0 {
			m.focus = focusTerminal
			m.sidebar.SetFocused(false)
			m.tuios.Mode = 1
		}
		return &m, nil

	case sidebar.QuitMsg:
		m.workspace.Save()
		if m.tuios != nil {
			m.tuios.Cleanup()
		}
		return &m, tea.Quit

	case sidebar.CleanExitMsg:
		tmuxKillAllCterm()
		m.workspace.Sessions = nil
		m.workspace.Save()
		if m.tuios != nil {
			m.tuios.Cleanup()
		}
		return &m, tea.Quit
	}

	// Bell check ticker
	if _, ok := msg.(bellCheckMsg); ok {
		alerted := checkTmuxBells(m.activeSession)
		for _, sid := range alerted {
			m.sidebar.SetAttention(sid)
			playBell()
		}
		if len(alerted) > 0 {
			m.sidebar.Refresh()
		}
		cmds = append(cmds, bellCheckTick())
		return &m, tea.Batch(cmds...)
	}

	if m.focus == focusTerminal {
		updated, cmd := m.tuios.Update(msg)
		m.tuios = updated.(*tuios.Model)
		cmds = append(cmds, cmd)
	}

	return &m, tea.Batch(cmds...)
}

// --- Input panel helpers ---

func (m *model) openInput(purpose inputPurpose, title, initialValue, context string) {
	m.inputPurpose = purpose
	m.inputContext = context
	m.focus = focusInput
	m.sidebar.SetFocused(false)
	m.inputPanel.Activate(inputpanel.ModeText, title, initialValue, m.width, inputPanelHeight)
}

func (m *model) openInputPath(purpose inputPurpose, title, initialValue string) {
	m.inputPurpose = purpose
	m.focus = focusInput
	m.sidebar.SetFocused(false)
	m.inputPanel.Activate(inputpanel.ModePath, title, initialValue, m.width, inputPanelHeight)
}

func (m model) handleInputSubmit(value string) (tea.Model, tea.Cmd) {
	switch m.inputPurpose {
	case purposeProjectPath:
		path := value
		if strings.HasPrefix(path, "~") {
			home, _ := os.UserHomeDir()
			path = home + path[1:]
		}
		m.pendingPath = path
		// Ask for name
		defaultName := path
		if idx := strings.LastIndex(strings.TrimSuffix(path, "/"), "/"); idx >= 0 {
			defaultName = strings.TrimSuffix(path[idx+1:], "/")
		}
		m.openInput(purposeProjectName, "Project name", defaultName, "")
		return &m, nil

	case purposeProjectName:
		if value != "" {
			m.workspace.AddProject(value, m.pendingPath)
			m.sidebar.Refresh()
		}
		m.inputPurpose = purposeNone
		m.pendingPath = ""
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil

	case purposeRenameProject:
		if value != "" && value != m.inputContext {
			m.workspace.RenameProject(m.inputContext, value)
			m.sidebar.Refresh()
		}
		m.inputPurpose = purposeNone
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil

	case purposeRenameSession:
		if value != "" && value != m.inputContext {
			m.workspace.RenameSession(m.inputContext, value)
			m.sidebar.Refresh()
		}
		m.inputPurpose = purposeNone
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil

	case purposeConfirmCloseSession:
		if value == "y" || value == "Y" {
			m.closeSession(m.inputContext)
		}
		m.inputPurpose = purposeNone
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil

	case purposeConfirmRemoveProject:
		if value == "y" || value == "Y" {
			for _, s := range m.workspace.SessionsForProject(m.inputContext) {
				m.closeSession(s.ID)
			}
			m.workspace.RemoveProject(m.inputContext)
			m.sidebar.Refresh()
		}
		m.inputPurpose = purposeNone
		m.focus = focusSidebar
		m.sidebar.SetFocused(true)
		return &m, nil
	}

	m.inputPurpose = purposeNone
	m.focus = focusSidebar
	m.sidebar.SetFocused(true)
	return &m, nil
}

// --- Session management ---

func (m *model) createSession(projectName string) (tea.Model, tea.Cmd) {
	session := m.workspace.CreateSession(projectName)
	if session == nil {
		return m, nil
	}

	tmuxName := tmuxPrefix + session.ID
	path := m.workspace.GetProjectPath(projectName)
	if path == "" {
		home, _ := os.UserHomeDir()
		path = home
	}
	tmuxCreateSession(tmuxName, path)

	m.attachToSession(session.ID)
	m.sidebar.SetSelected(session.ID)
	m.sidebar.Refresh()

	m.focus = focusTerminal
	m.sidebar.SetFocused(false)
	m.tuios.Mode = 1

	return m, nil
}

func (m *model) attachToSession(sessionID string) {
	tmuxName := tmuxPrefix + sessionID
	if !tmuxSessionExists(tmuxName) {
		return
	}

	if m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
		m.tuios.DeleteWindow(m.termWindowIdx)
		m.termWindowIdx = -1
	}

	m.tuios.AddWindow(sessionID)
	m.termWindowIdx = len(m.tuios.Windows) - 1
	if m.termWindowIdx < len(m.tuios.Windows) && m.tuios.Windows[m.termWindowIdx] != nil {
		m.tuios.Windows[m.termWindowIdx].SendInput(
			[]byte(fmt.Sprintf("tmux attach-session -t %s\n", tmuxName)),
		)
	}
	m.tuios.FocusWindow(m.termWindowIdx)
	m.activeSession = sessionID
}

func (m *model) closeSession(id string) {
	tmuxName := tmuxPrefix + id
	if tmuxSessionExists(tmuxName) {
		tmuxKillSession(tmuxName)
	}
	if m.activeSession == id {
		m.activeSession = ""
	}
	m.workspace.RemoveSession(id)
	m.sidebar.Refresh()
}

// --- View ---

func (m model) View() tea.View {
	if !m.ready {
		v := tea.NewView("Loading...")
		v.AltScreen = true
		return v
	}

	sidebarView := m.sidebar.View()
	tuiosView := m.tuios.View()

	sep := lipgloss.NewStyle().
		Faint(true).
		Render(strings.Repeat("│\n", m.height))

	mainHeight := m.height
	if m.inputPanel.Active() {
		mainHeight = m.height - inputPanelHeight
	}

	sidebarStyled := lipgloss.NewStyle().
		Width(sidebarWidth).
		Height(mainHeight).
		Render(sidebarView)

	topRow := lipgloss.JoinHorizontal(lipgloss.Top, sidebarStyled, sep, tuiosView.Content)

	var content string
	if m.inputPanel.Active() {
		panelView := m.inputPanel.View()
		content = topRow + "\n" + panelView
	} else {
		content = topRow
	}

	v := tea.NewView(content)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	return v
}

// --- Bell ---

func playBell() {
	sound := "/System/Library/Sounds/Tink.aiff"
	if _, err := os.Stat(sound); err == nil {
		exec.Command("afplay", "-v", "0.5", sound).Start()
	}
}

func main() {
	p := tea.NewProgram(
		newModel(),
		tea.WithFPS(60),
	)

	if _, err := p.Run(); err != nil {
		log.Fatal(err)
	}
}
