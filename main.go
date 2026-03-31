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

// Bell check result message (returned async)
type bellResultMsg struct {
	sessions []string
}

// Async bell check — runs tmux command in a Cmd so it doesn't block the event loop
func bellCheckCmd(activeSession string) tea.Cmd {
	return func() tea.Msg {
		time.Sleep(15 * time.Second)
		out, err := exec.Command("tmux", "list-sessions", "-F", "#{session_name} #{session_alerts}").Output()
		if err != nil {
			return bellResultMsg{}
		}
		var alerted []string
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
			if sessionID != activeSession && alerts != "" {
				alerted = append(alerted, sessionID)
			}
		}
		return bellResultMsg{sessions: alerted}
	}
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
	return tea.Batch(m.tuios.Init(), m.inputPanel.Init(), bellCheckCmd(m.activeSession))
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

	case tea.KeyPressMsg:
		key := msg.Key()

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
			switch key.Code {
			case tea.KeyLeft:
				m.focus = focusSidebar
				m.sidebar.SetFocused(true)
				m.tuios.Mode = 0
				return &m, nil
			case tea.KeyRight:
				if m.termWindowIdx >= 0 {
					m.focus = focusTerminal
					m.sidebar.SetFocused(false)
					m.tuios.Mode = 1
				}
				return &m, nil
			}
		}
		if key.Code == tea.KeySpace && key.Mod == tea.ModCtrl {
			m.prefixActive = true
			return &m, nil
		}

		if m.focus == focusSidebar {
			var cmd tea.Cmd
			m.sidebar, cmd = m.sidebar.Update(msg)
			cmds = append(cmds, cmd)
		} else if m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
			// Send keystrokes directly to the PTY — minimal path
			win := m.tuios.Windows[m.termWindowIdx]
			if win != nil {
				// Printable text — fastest path, no switch needed
				if key.Mod == 0 && key.Text != "" {
					win.SendInput([]byte(key.Text))
				} else if key.Mod == tea.ModCtrl {
					// Ctrl+letter → control character
					if key.Code >= 'a' && key.Code <= 'z' {
						win.SendInput([]byte{byte(key.Code) - 'a' + 1})
					}
				} else {
					// Special keys → escape sequences
					switch key.Code {
					case tea.KeyEnter:
						win.SendInput([]byte{'\r'})
					case tea.KeyBackspace:
						win.SendInput([]byte{0x7f})
					case tea.KeyTab:
						win.SendInput([]byte{'\t'})
					case tea.KeyEscape:
						win.SendInput([]byte{0x1b})
					case tea.KeyUp:
						win.SendInput([]byte("\x1b[A"))
					case tea.KeyDown:
						win.SendInput([]byte("\x1b[B"))
					case tea.KeyRight:
						win.SendInput([]byte("\x1b[C"))
					case tea.KeyLeft:
						win.SendInput([]byte("\x1b[D"))
					case tea.KeyHome:
						win.SendInput([]byte("\x1b[H"))
					case tea.KeyEnd:
						win.SendInput([]byte("\x1b[F"))
					case tea.KeyDelete:
						win.SendInput([]byte("\x1b[3~"))
					case tea.KeyPgUp:
						win.SendInput([]byte("\x1b[5~"))
					case tea.KeyPgDown:
						win.SendInput([]byte("\x1b[6~"))
					case tea.KeyF1:
						win.SendInput([]byte("\x1bOP"))
					case tea.KeyF2:
						win.SendInput([]byte("\x1bOQ"))
					case tea.KeyF3:
						win.SendInput([]byte("\x1bOR"))
					case tea.KeyF4:
						win.SendInput([]byte("\x1bOS"))
					case tea.KeyF5:
						win.SendInput([]byte("\x1b[15~"))
					case tea.KeyF6:
						win.SendInput([]byte("\x1b[17~"))
					case tea.KeyF7:
						win.SendInput([]byte("\x1b[18~"))
					case tea.KeyF8:
						win.SendInput([]byte("\x1b[19~"))
					case tea.KeyF9:
						win.SendInput([]byte("\x1b[20~"))
					case tea.KeyF10:
						win.SendInput([]byte("\x1b[21~"))
					case tea.KeyF11:
						win.SendInput([]byte("\x1b[23~"))
					case tea.KeyF12:
						win.SendInput([]byte("\x1b[24~"))
					}
				}
			}
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

	// --- Delayed tmux attach ---

	case attachReadyMsg:
		if m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
			win := m.tuios.Windows[m.termWindowIdx]
			if win != nil {
				win.SendInput([]byte(fmt.Sprintf("tmux attach-session -t %s\n", msg.tmuxName)))
			}
		}
		return &m, nil

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
		cmd := m.attachToSession(msg.ID)
		m.focus = focusTerminal
		m.sidebar.SetFocused(false)
		m.tuios.Mode = 1
		return &m, cmd

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

	// Bell check result (async)
	if result, ok := msg.(bellResultMsg); ok {
		for _, sid := range result.sessions {
			m.sidebar.SetAttention(sid)
			playBell()
		}
		if len(result.sessions) > 0 {
			m.sidebar.Refresh()
		}
		cmds = append(cmds, bellCheckCmd(m.activeSession))
		return &m, tea.Batch(cmds...)
	}

	// Always pass non-key messages to TUIOS so it processes PTY output
	updated, cmd := m.tuios.Update(msg)
	m.tuios = updated.(*tuios.Model)
	cmds = append(cmds, cmd)

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

	cmd := m.attachToSession(session.ID)
	m.sidebar.SetSelected(session.ID)
	m.sidebar.Refresh()

	m.focus = focusTerminal
	m.sidebar.SetFocused(false)
	m.tuios.Mode = 1

	return m, cmd
}

// Delayed attach message
type attachReadyMsg struct{ tmuxName string }

func (m *model) attachToSession(sessionID string) tea.Cmd {
	tmuxName := tmuxPrefix + sessionID
	if !tmuxSessionExists(tmuxName) {
		return nil
	}

	if m.termWindowIdx >= 0 && m.termWindowIdx < len(m.tuios.Windows) {
		m.tuios.DeleteWindow(m.termWindowIdx)
		m.termWindowIdx = -1
	}

	m.tuios.AddWindow(sessionID)
	m.termWindowIdx = len(m.tuios.Windows) - 1
	m.tuios.FocusWindow(m.termWindowIdx)
	m.activeSession = sessionID

	// Delay the tmux attach so the shell has time to start
	name := tmuxName
	return func() tea.Msg {
		time.Sleep(200 * time.Millisecond)
		return attachReadyMsg{tmuxName: name}
	}
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

	mainHeight := m.height
	if m.inputPanel.Active() {
		mainHeight = m.height - inputPanelHeight - 1
		if mainHeight < 5 {
			mainHeight = 5
		}
	}

	// Update sidebar height for this frame
	m.sidebar.SetSize(sidebarWidth, mainHeight)
	sidebarView := m.sidebar.View()

	rightWidth := m.width - sidebarWidth - 1

	// Right pane: TUIOS terminal or welcome screen
	var rightPane string
	if m.termWindowIdx >= 0 {
		tuiosView := m.tuios.View()
		rightPane = tuiosView.Content
	} else {
		rightPane = renderWelcome(rightWidth, mainHeight)
	}

	sep := lipgloss.NewStyle().
		Faint(true).
		Render(strings.Repeat("│\n", mainHeight))

	sidebarStyled := lipgloss.NewStyle().
		Width(sidebarWidth).
		Height(mainHeight).
		MaxHeight(mainHeight).
		Render(sidebarView)

	rightStyled := lipgloss.NewStyle().
		Width(rightWidth).
		Height(mainHeight).
		MaxHeight(mainHeight).
		Render(rightPane)

	topRow := lipgloss.JoinHorizontal(lipgloss.Top, sidebarStyled, sep, rightStyled)

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

func renderWelcome(width, height int) string {
	dim := lipgloss.NewStyle().Faint(true)
	green := lipgloss.NewStyle().Foreground(lipgloss.Color("#98be65"))
	pink := lipgloss.NewStyle().Foreground(lipgloss.Color("#ff6c6b"))
	yellow := lipgloss.NewStyle().Foreground(lipgloss.Color("#ECBE7B"))
	magenta := lipgloss.NewStyle().Foreground(lipgloss.Color("#c678dd"))
	cyan := lipgloss.NewStyle().Foreground(lipgloss.Color("#46D9FF"))
	frame := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("#ECBE7B"))

	fw := 35 // frame inner width
	fl := frame.Render("║")
	fr := frame.Render("║")
	pad := lipgloss.NewStyle().Width(fw)

	// Scene lines inside the painting frame (flower centered at pos ~14)
	scene := []string{
		"",
		"                " + magenta.Render("_") + pink.Render("\\"),
		"               " + pink.Render("(_)"),
		"           " + pink.Render("@") + "  " + magenta.Render("_|_") + "  " + pink.Render("@"),
		"          " + pink.Render("@@@") + green.Render(" / ") + pink.Render("@@@"),
		"           " + pink.Render("@") + green.Render("  |  ") + pink.Render("@"),
		green.Render("      ,       |") + "          " + yellow.Render("*"),
		green.Render("     ") + cyan.Render("/\\") + green.Render("       |      ") + yellow.Render("*") + green.Render("   |    ") + cyan.Render(","),
		green.Render("    /  \\   .  |    .   \\|/   / \\"),
		green.Render("   / .  \\  |\\ |    |\\   |  / . \\"),
		green.Render("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~"),
	}

	// Build framed painting
	var lines []string
	lines = append(lines, "")
	red := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("#ff6c6b"))
	lines = append(lines, red.Render("               a r t a"))
	lines = append(lines, dim.Render(" agent runtime terminal application"))
	lines = append(lines, "")
	lines = append(lines, frame.Render("╔"+strings.Repeat("═", fw)+"╗"))
	for _, s := range scene {
		lines = append(lines, fl+pad.Render(s)+fr)
	}
	lines = append(lines, frame.Render("╚"+strings.Repeat("═", fw)+"╝"))
	lines = append(lines, "")
	lines = append(lines, dim.Render("    Select a project and press 'n'"))
	lines = append(lines, dim.Render("    or press 'a' to add a project"))

	// Center vertically and horizontally
	frameWidth := fw + 2
	var b strings.Builder
	padTop := (height - len(lines)) / 2
	if padTop < 0 {
		padTop = 0
	}
	hPad := (width - frameWidth) / 2
	if hPad < 0 {
		hPad = 0
	}
	for i := 0; i < padTop; i++ {
		b.WriteString("\n")
	}
	for _, line := range lines {
		b.WriteString(strings.Repeat(" ", hPad) + line + "\n")
	}
	for i := padTop + len(lines); i < height; i++ {
		b.WriteString("\n")
	}

	return b.String()
}

func playBell() {
	sound := "/System/Library/Sounds/Tink.aiff"
	if _, err := os.Stat(sound); err == nil {
		exec.Command("afplay", "-v", "0.5", sound).Start()
	}
}

func main() {
	p := tea.NewProgram(
		newModel(),
		tea.WithFPS(120),
	)

	if _, err := p.Run(); err != nil {
		log.Fatal(err)
	}
}
