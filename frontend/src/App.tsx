import { Link, NavLink, Route, Routes } from 'react-router-dom';
import History from './pages/History';
import Pay from './pages/Pay';
import Send from './pages/Send';

export default function App() {
  return (
    <>
      <header className="top">
        <div className="wrap row-between">
          <Link to="/" className="brand">
            <span className="mark" aria-hidden="true" />
            SatsBridge
          </Link>
          <nav>
            <NavLink to="/" end>
              Send
            </NavLink>
            <NavLink to="/history">History</NavLink>
          </nav>
        </div>
      </header>
      <main className="wrap">
        <Routes>
          <Route path="/" element={<Send />} />
          <Route path="/pay/:id" element={<Pay />} />
          <Route path="/history" element={<History />} />
          <Route path="*" element={<p className="muted center">Page not found.</p>} />
        </Routes>
      </main>
      <footer className="wrap foot muted small">
        Sandbox demo. Not a licensed financial service. Don’t send money you can’t afford to lose.
      </footer>
    </>
  );
}
