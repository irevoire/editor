1. Create a ScreenBuffer type that represents everything currently printed on the screen
1.1 I should be able to share part of it safely for concurrent editing, and then block all editing when I want to read the result of everything
2. Create a View that can display itself inside of a ScreenBuffer
2.1 The "view" must handle the splits
2.2 The view must know which view is currently active
currently
2.3 The view also know the kind of all it's "sub-view"
3. Create a ViewBuffer, that's a specific kind of view that represent a Ropey: Rope of text we can edit
3.1 It must be able to display both the content of the gutter and the content of the rope
3.2 It holds the position of the cursor(s)
3.3 It's tied to a "buffer", but should not "own" the buffer. Other view may point and update the same buffer
