---
name: data-visualization
description: Design effective data visualizations: charts, graphs, dashboards, and infographics.
---

# Data Visualization

Create clear, accessible, and impactful data visualizations. This skill covers principles for designing charts, graphs, dashboards, and infographics that make data easy to understand and act on.

**What is Data Visualization?**
Data visualization is the practice of representing information visually, turning numbers, statistics, and data into charts, graphs, maps, and other visual formats that help people understand patterns, trends, and insights.

## When to Use Data Visualization

Create data visualizations when you need to:
- **Build dashboards** with charts and graphs
- **Create infographics** that tell a data story
- **Design analytics interfaces** for tracking metrics
- **Show relationships** between data points (network diagrams, correlation charts)
- **Display geographic data** (maps, choropleths)
- **Present trends over time** (line charts, area charts)
- **Compare values** (bar charts, column charts)
- **Show distributions** (histograms, box plots)

## Core Principles

### 1. Data-First Design
- Understand the data structure before designing
- Let the data inform the visualization type
- Consider data updates and transitions

### 2. Visual Encoding
- **Position**: Most accurate (x/y axes)
- **Length**: Bar charts, comparisons
- **Angle**: Pie charts (use sparingly)
- **Area**: Bubble charts
- **Color**: Categories, gradients for values

### 3. Accessibility
- Don't rely solely on color
- Add labels and tooltips
- Ensure keyboard navigation
- Provide text alternatives

## Chart Selection Guide

| Data Type | Recommended Chart |
|-----------|------------------|
| Comparison | Bar, Column |
| Trend over time | Line, Area |
| Part-to-whole | Stacked bar, Treemap |
| Distribution | Histogram, Box plot |
| Relationship | Scatter, Network |
| Geographic | Map, Choropleth |

## Choosing the Right Chart Type

**Start with your question**: What do you want to communicate?

- **"How much?"** → Bar chart, column chart
- **"Over time?"** → Line chart, area chart
- **"Parts of a whole?"** → Pie chart, stacked bar, treemap
- **"How are things related?"** → Scatter plot, bubble chart
- **"Where?"** → Map, choropleth
- **"How is data distributed?"** → Histogram, box plot

**Common Tools:**
- **D3.js**: Powerful JavaScript library for custom visualizations
- **Chart.js, Recharts, Victory**: Easier-to-use charting libraries
- **Tableau, Power BI**: Business intelligence tools
- **Figma, Illustrator**: For custom-designed infographics

## Implementation Pattern (D3.js Example)

```javascript
// Standard D3 pattern for custom visualizations
const svg = d3.select("#chart")
  .append("svg")
  .attr("viewBox", [0, 0, width, height]);

// Scales map data values to visual space
const x = d3.scaleLinear()
  .domain(d3.extent(data, d => d.value))
  .range([0, width]);

// Data binding: join data to DOM elements
svg.selectAll("rect")
  .data(data)
  .join("rect")
  .attr("x", d => x(d.value))
  .attr("height", d => y(d.category));
```

## Design Guidelines

### Color Palettes
- Use colorbrewer2.org for accessible palettes
- Limit to 5-7 colors for categories
- Use sequential palettes for continuous data

### Typography
- 12-14px for labels
- 16-18px for titles
- Use tabular figures for numbers
- Ensure contrast ratio ≥ 4.5:1

### Interactions
- Hover states for details
- Click to filter/focus
- Smooth transitions (300-500ms)
- Provide undo/reset options

### Responsive Design
- Use viewBox for scaling
- Adjust tick counts at breakpoints
- Consider mobile-specific layouts
- Touch-friendly targets (44px min)

## Data Visualization Best Practices

**Show the data clearly:**
- Remove unnecessary decoration (chart junk)
- Use consistent scales and axes
- Label axes and data points clearly
- Provide context (what does this mean?)

**Make it accessible:**
- Use sufficient color contrast
- Don't rely on color alone to convey meaning
- Add text labels and descriptions
- Provide alternative text for charts

**Help users explore:**
- Add interactive tooltips with details
- Allow filtering and drilling down
- Show related data on hover
- Provide clear legends and keys

## Common Patterns

### Tooltips
```javascript
element.on("mouseenter", (event, d) => {
  tooltip
    .style("opacity", 1)
    .html(`Value: ${d.value}`)
    .style("left", event.pageX + "px")
    .style("top", event.pageY + "px");
});
```

### Transitions
```javascript
bars.transition()
  .duration(400)
  .attr("height", d => y(d.value));
```

### Responsive
```javascript
const resize = () => {
  const width = container.clientWidth;
  svg.attr("width", width);
  x.range([0, width - margin.left - margin.right]);
  // Update elements
};
window.addEventListener("resize", resize);
```