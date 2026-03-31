package main

import (
	"fmt"
	"log"

	tea "charm.land/bubbletea/v2"
)

type model struct{ last string }

func (m model) Init() tea.Cmd { return nil }
func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		s := msg.String()
		if s == "q" {
			return m, tea.Quit
		}
		m.last = fmt.Sprintf("Key: %q  String(): %q  Type: %T", msg, s, msg)
	}
	return m, nil
}
func (m model) View() tea.View {
	return tea.NewView(fmt.Sprintf("Press keys (q to quit)\n\nLast: %s\n", m.last))
}

func main() {
	if _, err := tea.NewProgram(model{}).Run(); err != nil {
		log.Fatal(err)
	}
}
