# Chainlink Price Server

## What it does

Simple rust server that records Chainlink BTC-USD oracle websocket prices. When a client connects, it gives the client a history of past messages and relays live Chainlink messages.
This allows the client to start live timeseries modeling and forecasting right when they open the websocket rather than waiting for X minutes to get sufficient past data up to the current moment.
