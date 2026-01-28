# VHS tape file for recording Agent Box demo
# Run with: vhs demo.tape

Output demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 600
Set Padding 20
Set Theme "GruvboxDarkHard"

Type "# Agent Box - Git/JJ workspace management with Docker"
Enter
Sleep 1s

Type "# Show current repo info"
Enter
Sleep 500ms

Type "ab info"
Enter
Sleep 2s

Type "# Create a new jj workspace"
Enter
Sleep 500ms

Type "ab new -s demo-session"
Enter
Sleep 2s

Type "# Spawn a container for the workspace"
Enter
Sleep 500ms

Type "ab spawn -s demo-session --entrypoint pi"
Enter
Sleep 3s

Type "hello"
Enter
Sleep 2s

Type "/exit"
Enter
Sleep 1s

Type "# Or use local mode to work in current directory"
Enter
Sleep 500ms

Type "ab spawn -l --entrypoint pi"
Enter
Sleep 3s

Type "hello"
Enter
Sleep 2s

Type "/exit"
Enter
Sleep 500ms
