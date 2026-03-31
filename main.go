package main

import (
	"fmt"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/catalinj/c-term-tuios/sidebar"
	"github.com/catalinj/c-term-tuios/workspace"

	"github.com/Gaurav-Gosain/tuios/pkg/tuios"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

const sidebarWidth = 30
const tmuxPrefix = "cterm_"

type focus int

const (
	focusSidebar focus = iota
	focusTerminal
)

type inputMode int

const (
	inputNone inputMode = iota
	inputProjectPath
	inputProjectName
	inputRename
	inputConfirmClose
	inputConfirmRemove
)

type model struct {
	sidebar   sidebar.Model
	tuios     *tuios.Model
	workspace *workspace.Workspace
	focus     focus
	width     int
	height    int
	ready     bool

	// Text input state
	inputMode   inputMode
	inputBuffer string
	inputPrompt string
	inputContext string
	pendingPath string

	// The single TUIOS window index (we only ever have one window)
	termWindowIdx int
	activeSession string // currently attached tmux session ID
	prefixActive  bool   // true after Ctrl+Space, waiting for next key
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

	// Check which sessions are still alive
	alive := ws.Sessions[:0]
	for _, s := range ws.Sessions {
		tmuxName := tmuxPrefix + s.ID
		if tmuxSessionExists(tmuxName) {
			alive = append(alive, s)
		}
	}
	ws.Sessions = alive
	ws.Save()

	sb := sidebar.New(ws)

	t := tuios.New(
		tuios.WithTheme("doom-one"),
		tuios.WithAnimations(false),
		tuios.WithWorkspaces(1),
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
		workspace:     ws,
		focus:         focusSidebar,
		termWindowIdx: -1,
	}
}

func (m model) Init() tea.Cmd {
	return m.tuios.Init()
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
		updated, cmd := m.tuios.Update(tea.WindowSizeMsg{Width: tuiosWidth, Height: m.height})
		m.tuios = updated.(*tuios.Model)
		cmds = append(cmds, cmd)
		return &m, tea.Batch(cmds...)

	case tea.KeyMsg:
		if m.inputMode != inputNone {
			return m.handleInputKey(msg)
		}

		// Prefix key system: Ctrl+Space, then arrow
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
			// Unknown second key — fall through
		}

		// Detect prefix key (Ctrl+Space)
		if msg.String() == "ctrl+@" || msg.String() == "ctrl+space" || msg.String() == " " {
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
		m.inputMode = inputConfirmClose
		m.inputPrompt = fmt.Sprintf("Close %s? (y/n): ", msg.ID)
		m.inputContext = msg.ID
		m.inputBuffer = ""
		return &m, nil

	case sidebar.AddProjectMsg:
		home, _ := os.UserHomeDir()
		m.inputMode = inputProjectPath
		m.inputPrompt = "Project path: "
		m.inputBuffer = home + "/"
		return &m, nil

	case sidebar.RemoveProjectMsg:
		m.inputMode = inputConfirmRemove
		m.inputPrompt = fmt.Sprintf("Remove %s? (y/n): ", msg.Name)
		m.inputContext = msg.Name
		m.inputBuffer = ""
		return &m, nil

	case sidebar.RenameProjectMsg:
		m.inputMode = inputRename
		m.inputPrompt = "Rename project to: "
		m.inputBuffer = msg.Old
		m.inputContext = "project:" + msg.Old
		return &m, nil

	case sidebar.RenameSessionMsg:
		m.inputMode = inputRename
		m.inputPrompt = "Rename session to: "
		m.inputBuffer = msg.ID
		m.inputContext = "session:" + msg.ID
		return &m, nil

	case sidebar.MoveSessionMsg:
		m.workspace.SwapSessionInProject(msg.ID, msg.Direction)
		m.sidebar.Refresh()
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

	if m.focus == focusTerminal {
		updated, cmd := m.tuios.Update(msg)
		m.tuios = updated.(*tuios.Model)
		cmds = append(cmds, cmd)
	}

	return &m, tea.Batch(cmds...)
}

// --- Text input ---

func (m model) handleInputKey(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "esc", "escape":
		m.inputMode = inputNone
		return &m, nil
	case "enter":
		return m.handleInputSubmit()
	case "backspace":
		if len(m.inputBuffer) > 0 {
			m.inputBuffer = m.inputBuffer[:len(m.inputBuffer)-1]
		}
		return &m, nil
	case "tab":
		if m.inputMode == inputProjectPath {
			m.inputBuffer = completePath(m.inputBuffer)
		}
		return &m, nil
	default:
		if len(msg.String()) == 1 {
			m.inputBuffer += msg.String()
		}
		return &m, nil
	}
}

func completePath(input string) string {
	// Expand ~
	path := input
	if strings.HasPrefix(path, "~") {
		home, _ := os.UserHomeDir()
		path = home + path[1:]
	}

	dir := path
	prefix := ""

	// Split into directory and partial name
	if !strings.HasSuffix(path, "/") {
		dir = filepath.Dir(path)
		prefix = filepath.Base(path)
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return input
	}

	// Find matching directories
	var matches []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		name := e.Name()
		if strings.HasPrefix(strings.ToLower(name), strings.ToLower(prefix)) {
			matches = append(matches, name)
		}
	}

	if len(matches) == 0 {
		return input
	}

	if len(matches) == 1 {
		// Single match — complete it
		return filepath.Join(dir, matches[0]) + "/"
	}

	// Multiple matches — complete common prefix
	common := matches[0]
	for _, m := range matches[1:] {
		for i := range common {
			if i >= len(m) || common[i] != m[i] {
				common = common[:i]
				break
			}
		}
	}
	if len(common) > len(prefix) {
		return filepath.Join(dir, common)
	}

	return input
}

func listMatchingDirs(input string, maxLines int) []string {
	path := input
	if strings.HasPrefix(path, "~") {
		home, _ := os.UserHomeDir()
		path = home + path[1:]
	}

	dir := path
	prefix := ""
	if !strings.HasSuffix(path, "/") {
		dir = filepath.Dir(path)
		prefix = strings.ToLower(filepath.Base(path))
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}

	dimStyle := lipgloss.NewStyle().Faint(true)
	dirStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#51afef"))

	var lines []string
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, ".") {
			continue // skip hidden
		}
		if prefix != "" && !strings.HasPrefix(strings.ToLower(name), prefix) {
			continue
		}
		icon := " "
		style := dimStyle
		if e.IsDir() {
			icon = "/"
			style = dirStyle
		}
		line := style.Render(fmt.Sprintf("  %s%s", name, icon))
		lines = append(lines, line)
		if len(lines) >= maxLines {
			break
		}
	}
	return lines
}

func (m model) handleInputSubmit() (tea.Model, tea.Cmd) {
	switch m.inputMode {
	case inputProjectPath:
		path := m.inputBuffer
		if path == "" {
			m.inputMode = inputNone
			return &m, nil
		}
		if strings.HasPrefix(path, "~") {
			home, _ := os.UserHomeDir()
			path = home + path[1:]
		}
		m.pendingPath = path
		defaultName := path
		if idx := strings.LastIndex(path, "/"); idx >= 0 && idx < len(path)-1 {
			defaultName = path[idx+1:]
		}
		m.inputMode = inputProjectName
		m.inputPrompt = "Project name: "
		m.inputBuffer = defaultName
		return &m, nil

	case inputProjectName:
		name := m.inputBuffer
		if name == "" {
			m.inputMode = inputNone
			return &m, nil
		}
		m.workspace.AddProject(name, m.pendingPath)
		m.sidebar.Refresh()
		m.inputMode = inputNone
		m.pendingPath = ""
		return &m, nil

	case inputRename:
		newName := m.inputBuffer
		if newName != "" {
			if strings.HasPrefix(m.inputContext, "project:") {
				oldName := strings.TrimPrefix(m.inputContext, "project:")
				if newName != oldName {
					m.workspace.RenameProject(oldName, newName)
				}
			} else if strings.HasPrefix(m.inputContext, "session:") {
				oldID := strings.TrimPrefix(m.inputContext, "session:")
				if newName != oldID {
					m.workspace.RenameSession(oldID, newName)
				}
			}
			m.sidebar.Refresh()
		}
		m.inputMode = inputNone
		return &m, nil

	case inputConfirmClose:
		if m.inputBuffer == "y" || m.inputBuffer == "Y" {
			m.closeSession(m.inputContext)
		}
		m.inputMode = inputNone
		return &m, nil

	case inputConfirmRemove:
		if m.inputBuffer == "y" || m.inputBuffer == "Y" {
			for _, s := range m.workspace.SessionsForProject(m.inputContext) {
				m.closeSession(s.ID)
			}
			m.workspace.RemoveProject(m.inputContext)
			m.sidebar.Refresh()
		}
		m.inputMode = inputNone
		return &m, nil
	}

	m.inputMode = inputNone
	return &m, nil
}

// --- Session management (tmux-backed) ---

func (m *model) createSession(projectName string) (tea.Model, tea.Cmd) {
	session := m.workspace.CreateSession(projectName)
	if session == nil {
		return m, nil
	}

	// Create tmux session
	tmuxName := tmuxPrefix + session.ID
	path := m.workspace.GetProjectPath(projectName)
	if path == "" {
		home, _ := os.UserHomeDir()
		path = home
	}
	tmuxCreateSession(tmuxName, path)

	// Attach to it
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

	// Always destroy and recreate the TUIOS window so the tmux attach
	// command goes to a fresh shell, not into a running TUI app
	if m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
		m.tuios.DeleteWindow(m.termWindowIdx)
		m.termWindowIdx = -1
	}

	// Create a fresh window and attach to tmux
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

	// If this was the active session, detach
	if m.activeSession == id && m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
		win := m.tuios.Windows[m.termWindowIdx]
		if win != nil {
			// The tmux session just died, shell will return to prompt
			m.activeSession = ""
		}
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

	if m.inputMode != inputNone {
		inputStyle := lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#51afef"))
		dimStyle := lipgloss.NewStyle().Faint(true)
		prompt := inputStyle.Render(m.inputPrompt)
		cursor := m.inputBuffer + "█"

		// Build the entire sidebar replacement for input mode
		var inputView strings.Builder
		inputView.WriteString(dimStyle.Render(" "+strings.Repeat("─", sidebarWidth-2)) + "\n")
		inputView.WriteString(prompt + "\n")
		inputView.WriteString(cursor + "\n")
		inputView.WriteString(dimStyle.Render(" "+strings.Repeat("─", sidebarWidth-2)) + "\n")

		// Directory listing for path input
		if m.inputMode == inputProjectPath {
			dirEntries := listMatchingDirs(m.inputBuffer, m.height-8)
			for _, entry := range dirEntries {
				inputView.WriteString(entry + "\n")
			}
		}

		// Pad remaining space
		inputLines := strings.Count(inputView.String(), "\n")
		for i := inputLines; i < m.height-3; i++ {
			inputView.WriteString("\n")
		}
		inputView.WriteString(dimStyle.Render(" esc cancel  tab complete"))

		sidebarView = inputView.String()
	}

	tuiosView := m.tuios.View()

	sep := lipgloss.NewStyle().
		Faint(true).
		Render(strings.Repeat("│\n", m.height))

	sidebarStyled := lipgloss.NewStyle().
		Width(sidebarWidth).
		Height(m.height).
		Render(sidebarView)

	v := tea.NewView(lipgloss.JoinHorizontal(lipgloss.Top, sidebarStyled, sep, tuiosView.Content))
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
