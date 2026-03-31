package sidebar

import (
	"fmt"
	"os/exec"
	"strings"

	"github.com/catalinj/arta/workspace"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

// Messages sent from sidebar to parent
type SelectSessionMsg struct{ ID string }
type NewSessionMsg struct{ Project string }
type CloseSessionMsg struct{ ID string }
type AddProjectMsg struct{ Name, Path string }
type RemoveProjectMsg struct{ Name string }
type RenameProjectMsg struct{ Old, New string }
type RenameSessionMsg struct{ ID string }
type MoveProjectMsg struct{ Direction int } // -1 up, +1 down
type MoveSessionMsg struct{ ID string; Direction int }
type QuitMsg struct{}
type CleanExitMsg struct{}
type FocusTerminalMsg struct{}

type Model struct {
	workspace *workspace.Workspace
	width     int
	height    int
	cursor    int
	expanded  map[string]bool
	selected  string // selected session ID
	attention map[string]bool
	nerdFont  bool
	focused   bool

	// flattened list of visible items for cursor navigation
	items []item
}

type itemType int

const (
	itemProject itemType = iota
	itemSession
)

type item struct {
	typ     itemType
	name    string // project name or session ID
	project string // parent project name (for sessions)
}

func New(ws *workspace.Workspace) Model {
	m := Model{
		workspace: ws,
		expanded:  make(map[string]bool),
		attention: make(map[string]bool),
		nerdFont:  detectNerdFont(),
		focused:   true,
	}
	// Expand all projects by default
	for _, p := range ws.Projects {
		m.expanded[p.Name] = true
	}
	m.rebuildItems()
	return m
}

func (m *Model) SetSize(w, h int) {
	m.width = w
	m.height = h
}

func (m *Model) SetFocused(f bool) {
	m.focused = f
}

func (m *Model) SetSelected(id string) {
	m.selected = id
	delete(m.attention, id)
}

func (m *Model) SetAttention(id string) {
	if id != m.selected {
		m.attention[id] = true
	}
}

func (m *Model) ClearAttention(id string) {
	delete(m.attention, id)
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.MouseClickMsg:
		if !m.focused {
			return m, nil
		}
		mouse := msg.Mouse()
		// Convert Y position to item index (accounting for header lines)
		clickLine := mouse.Y
		// Find which item is at this line
		line := 0
		for i, it := range m.items {
			// Each project has a blank line before it + the project line
			if it.typ == itemProject {
				line += 2 // blank line + project line
			} else {
				line++ // session line
			}
			// Add 2 for header ("c-term" + separator)
			if line+2 >= clickLine && clickLine > 0 {
				m.cursor = i
				return m.handleAction()
			}
		}
		return m, nil

	case tea.KeyMsg:
		if !m.focused {
			return m, nil
		}
		switch msg.String() {
		case "j", "down":
			if m.cursor < len(m.items)-1 {
				m.cursor++
			}
		case "k", "up":
			if m.cursor > 0 {
				m.cursor--
			}
		case "enter":
			return m.handleAction()
		case "l":
			return m, func() tea.Msg { return FocusTerminalMsg{} }
		case "tab":
			return m.handleToggle()
		case "a":
			return m, func() tea.Msg { return AddProjectMsg{} }
		case "n":
			return m.handleNewSession()
		case "d":
			return m.handleCloseSession()
		case "D":
			return m.handleRemoveProject()
		case "r":
			return m.handleRename()
		case "J":
			return m.handleMoveDown()
		case "K":
			return m.handleMoveUp()
		case "q":
			return m, func() tea.Msg { return QuitMsg{} }
		case "Q":
			return m, func() tea.Msg { return CleanExitMsg{} }
		}
	}
	return m, nil
}

func (m Model) handleAction() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	switch it.typ {
	case itemProject:
		return m.handleToggle()
	case itemSession:
		return m, func() tea.Msg { return SelectSessionMsg{ID: it.name} }
	}
	return m, nil
}

func (m Model) handleToggle() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		if m.expanded[it.name] {
			delete(m.expanded, it.name)
		} else {
			m.expanded[it.name] = true
		}
		m.rebuildItems()
	}
	return m, nil
}

func (m Model) handleNewSession() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	proj := ""
	switch it.typ {
	case itemProject:
		proj = it.name
	case itemSession:
		proj = it.project
	}
	if proj != "" {
		return m, func() tea.Msg { return NewSessionMsg{Project: proj} }
	}
	return m, nil
}

func (m Model) handleCloseSession() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemSession {
		return m, func() tea.Msg { return CloseSessionMsg{ID: it.name} }
	}
	return m, nil
}

func (m Model) handleRemoveProject() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		return m, func() tea.Msg { return RemoveProjectMsg{Name: it.name} }
	}
	return m, nil
}

func (m Model) handleRename() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		name := it.name
		return m, func() tea.Msg { return RenameProjectMsg{Old: name} }
	}
	if it.typ == itemSession {
		id := it.name
		return m, func() tea.Msg { return RenameSessionMsg{ID: id} }
	}
	return m, nil
}

func (m Model) handleMoveDown() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		return m.handleMoveProject(1)
	}
	if it.typ == itemSession {
		id := it.name
		return m, func() tea.Msg { return MoveSessionMsg{ID: id, Direction: 1} }
	}
	return m, nil
}

func (m Model) handleMoveUp() (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		return m.handleMoveProject(-1)
	}
	if it.typ == itemSession {
		id := it.name
		return m, func() tea.Msg { return MoveSessionMsg{ID: id, Direction: -1} }
	}
	return m, nil
}

func (m *Model) GetCursorProject() string {
	if m.cursor >= len(m.items) {
		return ""
	}
	it := m.items[m.cursor]
	switch it.typ {
	case itemProject:
		return it.name
	case itemSession:
		return it.project
	}
	return ""
}

func (m Model) handleMoveProject(dir int) (Model, tea.Cmd) {
	if m.cursor >= len(m.items) {
		return m, nil
	}
	it := m.items[m.cursor]
	if it.typ == itemProject {
		return m, func() tea.Msg { return MoveProjectMsg{Direction: dir} }
	}
	return m, nil
}

func (m *Model) rebuildItems() {
	m.items = nil
	for _, p := range m.workspace.Projects {
		m.items = append(m.items, item{typ: itemProject, name: p.Name})
		if m.expanded[p.Name] {
			for _, s := range m.workspace.SessionsForProject(p.Name) {
				m.items = append(m.items, item{typ: itemSession, name: s.ID, project: p.Name})
			}
		}
	}
	if m.cursor >= len(m.items) && len(m.items) > 0 {
		m.cursor = len(m.items) - 1
	}
}

func (m *Model) Refresh() {
	m.rebuildItems()
}

func (m Model) View() string {
	var b strings.Builder

	headerStyle := lipgloss.NewStyle().Bold(true)
	dimStyle := lipgloss.NewStyle().Faint(true)

	b.WriteString(headerStyle.Render(" ARTA"))
	b.WriteString("\n")
	b.WriteString(dimStyle.Render(" " + strings.Repeat("─", m.width-2)))
	b.WriteString("\n")

	if len(m.workspace.Projects) == 0 {
		b.WriteString(dimStyle.Render("\n  No projects yet.\n  Press 'a' to add one."))
		b.WriteString("\n")
	} else {
		idx := 0
		for _, p := range m.workspace.Projects {
			arrow := "▶"
			if m.expanded[p.Name] {
				arrow = "▼"
			}
			if m.nerdFont {
				if m.expanded[p.Name] {
					arrow = "\uf115"
				} else {
					arrow = "\uf114"
				}
			}

			sessions := m.workspace.SessionsForProject(p.Name)
			count := len(sessions)

			line := fmt.Sprintf(" %s %s", arrow, p.Name)
			if count > 0 {
				line += dimStyle.Render(fmt.Sprintf(" (%d)", count))
			}

			style := lipgloss.NewStyle().Bold(true)
			if m.focused && m.cursor == idx {
				style = style.Reverse(true)
			}
			b.WriteString("\n")
			b.WriteString(style.Render(line))
			b.WriteString("\n")
			idx++

			if m.expanded[p.Name] {
				if len(sessions) == 0 {
					b.WriteString(dimStyle.Render("    (no sessions)"))
					b.WriteString("\n")
				}
				for _, s := range sessions {
					icon := "○"
					color := lipgloss.Color("#a9a1e1")

					if s.ID == m.selected {
						icon = "●"
						color = lipgloss.Color("#51afef")
					}
					if m.attention[s.ID] {
						icon = "*"
						color = lipgloss.Color("#ff6c6b")
						if m.nerdFont {
							icon = "\uf0f3"
						}
					}
					if s.ID == m.selected && m.nerdFont {
						icon = "\uf120"
					} else if m.nerdFont && !m.attention[s.ID] {
						icon = "\uf489"
					}

					sLine := fmt.Sprintf("   %s %s", icon, s.ID)
					sStyle := lipgloss.NewStyle().Foreground(color)
					if s.ID == m.selected {
						sStyle = sStyle.Bold(true)
					}
					if m.focused && m.cursor == idx {
						sStyle = sStyle.Reverse(true)
					}

					b.WriteString(sStyle.Render(sLine))
					b.WriteString("\n")
					idx++
				}
			}
		}
	}

	// Footer
	b.WriteString(dimStyle.Render("\n " + strings.Repeat("─", m.width-2)))
	b.WriteString("\n")

	bold := lipgloss.NewStyle().Bold(true)
	b.WriteString(bold.Render(" a") + dimStyle.Render(" add") + "  ")
	b.WriteString(bold.Render("D") + dimStyle.Render(" remove") + "\n")
	b.WriteString(bold.Render(" n") + dimStyle.Render(" new session") + "\n")
	b.WriteString(bold.Render(" d") + dimStyle.Render(" close session") + "\n")
	b.WriteString(bold.Render(" r") + dimStyle.Render(" rename") + "  ")
	b.WriteString(bold.Render("J/K") + dimStyle.Render(" reorder") + "\n")
	b.WriteString(bold.Render(" q") + dimStyle.Render(" quit") + "  ")
	b.WriteString(bold.Render("Q") + dimStyle.Render(" clean exit") + "\n")

	// Pad to full height
	lines := strings.Count(b.String(), "\n")
	for i := lines; i < m.height-1; i++ {
		b.WriteString("\n")
	}

	return b.String()
}

func detectNerdFont() bool {
	out, err := exec.Command("fc-list").Output()
	if err != nil {
		return false
	}
	return strings.Contains(string(out), "Nerd Font")
}
