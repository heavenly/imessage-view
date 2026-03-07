# iMessage UI Redesign Plan

## Overview
Transform the current web interface into a pixel-perfect iMessage clone. The design should replicate macOS/iOS iMessage app styling exactly.

## Current State Analysis
- **Framework**: Rust (Axum) + Askama templates + HTMX + Pico CSS
- **Structure**: Base layout with navigation, container-based content
- **Styling**: Basic Pico CSS with custom overrides in style.css
- **Pages**: Index (conversations), Conversation detail, Search, Attachments, Analytics

## Target Design Reference: macOS iMessage

### Key iMessage Visual Elements

#### Color Palette (macOS iMessage)
```
--im-bg-primary: #ffffff (main background)
--im-bg-secondary: #f5f5f7 (sidebar/secondary areas)
--im-bg-tertiary: #e8e8ed (hover states, separators)
--im-bg-messages: #ffffff (message area background)

--im-bubble-sent: #007aff (sent message bubble - iMessage blue)
--im-bubble-sent-gradient: linear-gradient(180deg, #007aff 0%, #0051d5 100%)
--im-bubble-received: #e5e5ea (received message bubble - gray)
--im-bubble-received-gradient: linear-gradient(180deg, #f2f2f7 0%, #e5e5ea 100%)

--im-text-primary: #000000
--im-text-secondary: #8e8e93
--im-text-tertiary: #c7c7cc
--im-text-inverse: #ffffff (on blue bubbles)

--im-accent: #007aff (links, active states)
--im-border: #d1d1d6
--im-divider: rgba(0,0,0,0.1)

--im-green: #34c759 (SMS messages)
--im-tapback-blue: #007aff
--im-tapback-gray: #8e8e93
```

#### Typography
- **Font Family**: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif
- **Conversation Name**: 13px, font-weight: 600
- **Message Preview**: 12px, font-weight: 400, color: secondary
- **Timestamp**: 11px, font-weight: 400, color: tertiary
- **Message Body**: 14px, line-height: 1.4
- **Header Name**: 15px, font-weight: 600

#### Layout Structure

**Main Layout (Index Page - Sidebar Style)**
```
┌─────────────────────────────────────────────┐
│  iMessage                    [Search Bar]   │  ← Header (56px height)
├─────────────────────────────────────────────┤
│ ┌─────────────────┬─────────────────────────┤
│ │                 │                         │
│ │  Conversation   │  Empty State /          │
│ │     List        │  Selected Conversation  │
│ │   (320px)       │      Preview            │
│ │                 │                         │
│ │  [Avatar] Name  │                         │
│ │  Preview text   │                         │
│ │  Time           │                         │
│ │                 │                         │
│ └─────────────────┴─────────────────────────┤
└─────────────────────────────────────────────┘
```

**Conversation Page Layout**
```
┌─────────────────────────────────────────────┐
│ ←  [Avatar]  Contact Name          [Info]   │  ← Header
├─────────────────────────────────────────────┤
│                                             │
│           Yesterday, 2:30 PM               │  ← Date separator
│                                             │
│    ┌─────────────┐                          │
│    │ Message     │                          │  ← Received (left)
│    │ text here   │                          │
│    └─────────────┘                          │
│                                             │
│                          ┌─────────────┐    │
│                          │ Message     │    │  ← Sent (right, blue)
│                          │ text here   │    │
│                          └─────────────┘    │
│                                             │
│    ┌─────────────┐                          │
│    │ [Image]     │                          │  ← Attachment
│    └─────────────┘                          │
│                                             │
├─────────────────────────────────────────────┤
│  [Camera]  [Text Input...]  [Emoji] [Send]  │  ← Input bar
└─────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Base Layout & Foundation
**Files to modify:**
- `templates/base.html` - Complete restructuring
- `static/css/style.css` - New CSS architecture

**Changes:**
1. Replace Pico CSS dependency with custom iMessage CSS (or keep minimal)
2. Create new base layout with proper iMessage container structure
3. Implement CSS custom properties (variables) for all iMessage colors
4. Add -apple-system font stack
5. Remove Pico CSS classes, replace with iMessage-specific classes

### Phase 2: Conversation List (Index Page)
**Files to modify:**
- `templates/index.html` - Restructure to sidebar layout
- `templates/partials/conversations.html` - Redesign conversation cards
- `static/css/style.css` - Add conversation list styles

**iMessage Sidebar Features:**
1. Fixed-width sidebar (300-320px)
2. Search bar at top with iMessage styling
3. Conversation cards with:
   - Circular avatar (40px) with contact initials or group icon
   - Contact name (bold, truncated)
   - Message preview (gray, truncated, single line)
   - Timestamp (right-aligned, gray)
   - Subtle separator lines between items
   - Hover state (light gray background)
   - Active state (blue tint/background)
4. Smooth scrolling

### Phase 3: Conversation Header
**Files to modify:**
- `templates/conversation.html` - Redesign header
- `static/css/style.css` - Header styles

**iMessage Header Features:**
1. Back button (←) on left
2. Centered contact info:
   - Avatar (44px)
   - Contact name (bold)
   - Participants or status (gray, smaller)
3. Optional info button on right
4. Subtle bottom border/shadow
5. Fixed position at top

### Phase 4: Message Bubbles (Critical)
**Files to modify:**
- `templates/partials/messages.html` - Restructure message HTML
- `static/css/style.css` - Message bubble styles

**iMessage Bubble Features:**
1. **Bubble Shape:**
   - Rounded corners (18-20px radius)
   - Special tail/corner on one side (CSS ::before pseudo-element)
   - Sent bubbles: tail on bottom-right
   - Received bubbles: tail on bottom-left

2. **Sent Bubbles (Blue):**
   - Background: #007aff gradient
   - Text: white
   - Align: right (flex-end)
   - Max-width: 70% of container
   - Padding: 10px 14px

3. **Received Bubbles (Gray):**
   - Background: #e5e5ea gradient
   - Text: black
   - Align: left (flex-start)
   - Max-width: 70% of container
   - Padding: 10px 14px

4. **Tail Implementation (CSS):**
```css
.message-bubble.sent::before {
  content: '';
  position: absolute;
  bottom: 0;
  right: -8px;
  width: 16px;
  height: 16px;
  background: inherit;
  border-bottom-left-radius: 14px;
  /* Mask to create tail shape */
}
```

5. **Message Grouping:**
   - First message in group: normal corners except tail side
   - Middle messages: rounded corners, no tail
   - Last message: tail on appropriate side
   - Gap between groups: 12px
   - Gap within group: 2px

### Phase 5: Date Separators & Timestamps
**Files to modify:**
- `templates/partials/messages.html`
- `static/css/style.css`

**Features:**
1. Date separators between days:
   - Centered text
   - Gray background pill
   - Text: "Today", "Yesterday", "Monday, January 1"
   - Font: 11px, gray

2. Message timestamps:
   - Inside bubble for last message in group
   - Small, slightly transparent
   - Format: "2:30 PM" or "Yesterday 2:30 PM"

### Phase 6: Attachments & Media
**Files to modify:**
- `templates/partials/messages.html` - Attachment rendering
- `templates/partials/conversation_attachments.html`
- `static/css/style.css`

**Features:**
1. Image attachments:
   - Rounded corners (12px)
   - Max-width within bubble
   - Maintain aspect ratio
   - Loading state

2. File attachments:
   - Icon + filename layout
   - Download button styling

3. Conversation attachments grid:
   - iMessage-style grid layout
   - Thumbnail previews
   - Click to expand modal

### Phase 7: Input Area & Details
**Files to modify:**
- `templates/conversation.html` - Add input bar
- `static/css/style.css` - Input styling

**Features:**
1. Fixed input bar at bottom
2. Camera/photo button (left)
3. Text input field (center):
   - Rounded corners
   - Gray background
   - Placeholder: "iMessage"
4. Send button (right):
   - Blue circle with arrow icon
   - Only visible when typing

5. Additional details:
   - Message status indicators (delivered, read)
   - Tapback reactions (optional enhancement)
   - Typing indicator animation

### Phase 8: Responsive & Polish
**Files to modify:**
- All CSS files

**Features:**
1. Mobile responsive:
   - Full-screen conversation view
   - Slide-in sidebar on mobile
   - Touch-friendly hit areas

2. Animations:
   - Message send animation
   - Smooth scroll
   - Hover transitions
   - Loading states

3. Dark mode support (optional):
   - Media query for prefers-color-scheme
   - Inverted color palette

## Technical Implementation Notes

### HTML Structure Changes

**Current message structure:**
```html
<div class="message sent">
  <div class="message-bubble">
    <p class="message-body">Text</p>
    <span class="message-meta">Time</span>
  </div>
</div>
```

**New iMessage structure:**
```html
<div class="message-group sent">
  <div class="message">
    <div class="message-bubble">
      <p class="message-text">Text</p>
      <span class="message-time">2:30 PM</span>
    </div>
  </div>
</div>
```

### CSS Architecture
```css
/* 1. Variables */
:root { /* all colors, spacing */ }

/* 2. Reset & Base */
body, html { /* iMessage base styles */ }

/* 3. Layout */
.im-container { }
.im-sidebar { }
.im-chat-area { }

/* 4. Components */
.im-header { }
.im-conversation-list { }
.im-conversation-item { }
.im-message-group { }
.im-message { }
.im-bubble { }
.im-bubble--sent { }
.im-bubble--received { }
.im-input-bar { }

/* 5. Utilities */
.im-text-truncate { }
.im-avatar { }
```

### Required Template Updates

**base.html:**
- Remove Pico CSS link
- Add new CSS architecture
- Restructure body layout

**index.html:**
- Two-column layout (sidebar + main)
- Sticky search bar in sidebar

**conversation.html:**
- Fixed header
- Scrollable message area
- Fixed input bar at bottom

**partials/messages.html:**
- Group messages by sender
- Add date separators
- Implement bubble tails

**partials/conversations.html:**
- Avatar + content layout
- Proper truncation
- Active state handling

## Success Criteria
- [ ] Visual match to macOS iMessage at 95%+ accuracy
- [ ] All existing functionality preserved
- [ ] Responsive on mobile devices
- [ ] Smooth animations and transitions
- [ ] Proper message grouping and timestamps
- [ ] Working attachment previews
- [ ] Clean, maintainable CSS code

## Risk Mitigation
1. **Keep backups** - Save current templates before editing
2. **Incremental changes** - Phase-by-phase implementation
3. **Test frequently** - Verify after each phase
4. **Fallback option** - Keep original CSS commented out initially

## Dependencies
- No new npm packages needed
- Using pure CSS for styling
- HTMX already in place for interactivity
- May need custom SVG for icons (camera, send arrow, etc.)
